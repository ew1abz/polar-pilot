//! Polar Pilot — antenna rotator controller firmware
//!
//! Async tasks on Embassy: motor control, OLED display, 5-way navigation,
//! rotctld TCP server, EasyComm II serial, W5500 Ethernet, heartbeat LED.

#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::exti::ExtiInput;
use embassy_stm32::gpio::{Input, Level, Output, OutputType, Pull, Speed};
use embassy_stm32::i2c::I2c;
use embassy_stm32::spi::{self, Spi};
use embassy_stm32::time::Hertz;
use embassy_stm32::timer::simple_pwm::{PwmPin, SimplePwm};
use embassy_stm32::usart;
use embassy_time::Timer;
use embedded_hal_bus::spi::ExclusiveDevice;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_reset as _};

mod tasks;
mod types;
mod util;

use tasks::display::display_task;
use tasks::easycom::easycom_task;
use tasks::keys::key_task;
use tasks::motor::{motor_task, STEP_HZ};
use tasks::net::{dhcp_watchdog_task, ethernet_task, led_task, net_task};
use tasks::rotctld::rotctld_task;
use types::{RotatorState, STATE};

// ── Interrupt bindings ──────────────────────────────────────────────

embassy_stm32::bind_interrupts!(struct Irqs {
    DMA1_CHANNEL2 => embassy_stm32::dma::InterruptHandler<embassy_stm32::peripherals::DMA1_CH2>;
    DMA1_CHANNEL3 => embassy_stm32::dma::InterruptHandler<embassy_stm32::peripherals::DMA1_CH3>;
    DMA1_CHANNEL6 => embassy_stm32::dma::InterruptHandler<embassy_stm32::peripherals::DMA1_CH6>;
    DMA1_CHANNEL7 => embassy_stm32::dma::InterruptHandler<embassy_stm32::peripherals::DMA1_CH7>;
    EXTI3 => embassy_stm32::exti::InterruptHandler<embassy_stm32::interrupt::typelevel::EXTI3>;
    RNG => embassy_stm32::rng::InterruptHandler<embassy_stm32::peripherals::RNG>;
    USART2 => embassy_stm32::usart::InterruptHandler<embassy_stm32::peripherals::USART2>;
});

// ── Main: init + spawn ──────────────────────────────────────────────

#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
    let mut config = embassy_stm32::Config::default();
    {
        use embassy_stm32::rcc::*;
        config.rcc.hsi = true;
        config.rcc.pll = Some(Pll {
            source: PllSource::HSI,
            prediv: PllPreDiv::DIV1,
            mul: PllMul::MUL10,
            divp: None,
            divq: None,
            divr: Some(PllRDiv::DIV2),
        });
        config.rcc.sys = Sysclk::PLL1_R;
        config.rcc.hsi48 = Some(Hsi48Config {
            sync_from_usb: false,
        });
    }
    let p = embassy_stm32::init(config);

    info!("Polar Pilot starting...");

    // Publish initial state
    STATE.sender().send(RotatorState::default());

    // ── LED heartbeat ───────────────────────────────────────────
    let led = Output::new(p.PB3, Level::High, Speed::Low);
    spawner.spawn(unwrap!(led_task(led)));

    // ── OLED display (I2C1) — spawned after stack so it can read IP ──
    let i2c = I2c::new_blocking(p.I2C1, p.PB6, p.PB7, Default::default());

    // ── W5500 Ethernet ──────────────────────────────────────────
    let mut w5500_rst = Output::new(p.PA1, Level::Low, Speed::Low);
    Timer::after_millis(1).await;
    w5500_rst.set_high();
    Timer::after_millis(2).await;

    let mut spi_cfg = spi::Config::default();
    spi_cfg.frequency = Hertz(1_000_000);
    let spi = Spi::new(
        p.SPI1, p.PA5, p.PA7, p.PA6, p.DMA1_CH3, p.DMA1_CH2, Irqs, spi_cfg,
    );
    let cs = Output::new(p.PA4, Level::High, Speed::VeryHigh);
    let spi_dev = ExclusiveDevice::new(spi, cs, embassy_time::Delay).unwrap();

    let w5500_int = ExtiInput::new(p.PA3, p.EXTI3, Pull::Up, Irqs);
    let mac = [0x02, 0x00, 0x00, 0x00, 0x00, 0x01];

    static W5500_STATE: StaticCell<embassy_net_wiznet::State<2, 2>> = StaticCell::new();
    let w5500_state = W5500_STATE.init(embassy_net_wiznet::State::<2, 2>::new());

    let (device, w5500_runner) =
        embassy_net_wiznet::new(mac, w5500_state, spi_dev, w5500_int, w5500_rst)
            .await
            .unwrap();

    let net_config = embassy_net::Config::dhcpv4(Default::default());
    let mut rng = embassy_stm32::rng::Rng::new(p.RNG, Irqs);
    let seed = rng.next_u32() as u64 | ((rng.next_u32() as u64) << 32);

    static NET_RESOURCES: StaticCell<embassy_net::StackResources<3>> = StaticCell::new();
    let (stack, net_runner) = embassy_net::new(
        device,
        net_config,
        NET_RESOURCES.init(embassy_net::StackResources::new()),
        seed,
    );

    spawner.spawn(unwrap!(ethernet_task(w5500_runner)));
    spawner.spawn(unwrap!(net_task(net_runner)));
    spawner.spawn(unwrap!(dhcp_watchdog_task(stack)));
    spawner.spawn(unwrap!(display_task(i2c, stack)));

    // ── Stepper motors ──────────────────────────────────────────
    let motor_en = Output::new(p.PB4, Level::High, Speed::Low);
    let az_dir = Output::new(p.PC14, Level::Low, Speed::Low);
    let el_dir = Output::new(p.PA9, Level::Low, Speed::Low);
    let az_home = Input::new(p.PA10, Pull::Up);
    let el_home = Input::new(p.PB5, Pull::Up);

    let el_step = PwmPin::new(p.PA0, OutputType::PushPull);
    let el_pwm = SimplePwm::new(
        p.TIM2,
        Some(el_step),
        None,
        None,
        None,
        Hertz(STEP_HZ),
        Default::default(),
    );

    let az_step = PwmPin::new(p.PA8, OutputType::PushPull);
    let az_pwm = SimplePwm::new(
        p.TIM1,
        Some(az_step),
        None,
        None,
        None,
        Hertz(STEP_HZ),
        Default::default(),
    );

    spawner.spawn(unwrap!(motor_task(
        az_pwm, el_pwm, az_dir, el_dir, motor_en, az_home, el_home
    )));

    // ── 5-way navigation buttons ────────────────────────────────
    let btn_up = Input::new(p.PB1, Pull::Up);
    let btn_down = Input::new(p.PB0, Pull::Up);
    let btn_left = Input::new(p.PA11, Pull::Up);
    let btn_right = Input::new(p.PA12, Pull::Up);
    let btn_center = Input::new(p.PC15, Pull::Up);
    spawner.spawn(unwrap!(key_task(
        btn_up, btn_down, btn_left, btn_right, btn_center
    )));

    // ── Rotctld TCP server (2 concurrent clients) ───────────────
    spawner.spawn(unwrap!(rotctld_task(stack)));
    spawner.spawn(unwrap!(rotctld_task(stack)));

    // ── EasyComm II serial (USART2) ─────────────────────────────
    let mut usart_cfg = usart::Config::default();
    usart_cfg.baudrate = 9600;
    let uart = usart::Uart::new(
        p.USART2, p.PA15, p.PA2, p.DMA1_CH7, p.DMA1_CH6, Irqs, usart_cfg,
    )
    .unwrap();
    let (usart_tx, usart_rx) = uart.split();
    spawner.spawn(unwrap!(easycom_task(usart_rx, usart_tx)));

    info!("All tasks spawned");

    // Main task idles
    loop {
        Timer::after_secs(3600).await;
    }
}
