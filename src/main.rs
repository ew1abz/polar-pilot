//! Polar Pilot — antenna rotator controller firmware
//!
//! Async tasks on Embassy: motor control, OLED display, 5-way navigation,
//! rotctld TCP server, EasyComm II serial, W5500 Ethernet, heartbeat LED.

#![no_std]
#![no_main]

use core::fmt::Write as FmtWrite;

use defmt::*;
use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_stm32::exti::ExtiInput;
use embassy_stm32::gpio::{Input, Level, Output, OutputType, Pull, Speed};
use embassy_stm32::i2c::I2c;
use embassy_stm32::spi::{self, Spi};
use embassy_stm32::time::Hertz;
use embassy_stm32::timer::simple_pwm::{PwmPin, SimplePwm};
use embassy_stm32::usart;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::watch::Watch;
use embassy_time::{Duration, Ticker, Timer};
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::mono_font::MonoTextStyleBuilder;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Circle, Line, PrimitiveStyle};
use embedded_graphics::text::Text;
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_io_async::Write as AsyncWrite;
use ssd1306::prelude::*;
use ssd1306::rotation::DisplayRotation;
use ssd1306::size::DisplaySize128x64;
use ssd1306::I2CDisplayInterface;
use ssd1306::Ssd1306;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

// ── Shared types ────────────────────────────────────────────────────

#[derive(Clone, Copy, Format)]
pub struct RotatorState {
    pub target_az: f32,
    pub target_el: f32,
    pub current_az: f32,
    pub current_el: f32,
    pub moving: bool,
    pub link_up: bool,
}

impl Default for RotatorState {
    fn default() -> Self {
        Self {
            target_az: 0.0,
            target_el: 0.0,
            current_az: 0.0,
            current_el: 0.0,
            moving: false,
            link_up: false,
        }
    }
}

#[derive(Clone, Copy, Format)]
pub enum RotatorCmd {
    GoTo { az: f32, el: f32 },
    Stop,
}

// ── Global shared primitives ────────────────────────────────────────

static STATE: Watch<CriticalSectionRawMutex, RotatorState, 4> = Watch::new();
static CMD: Channel<CriticalSectionRawMutex, RotatorCmd, 4> = Channel::new();

// ── Motor constants ─────────────────────────────────────────────────

const SLEW_RATE: f32 = 1.0;
const MOTOR_TICK: Duration = Duration::from_millis(10);
const STEP_HZ: u32 = 1_000;
const POS_EPSILON: f32 = 0.05;

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

// ── Helpers ─────────────────────────────────────────────────────────

fn parse_f32(s: &str) -> Option<f32> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let bytes = s.as_bytes();
    let (negative, chars) = if bytes[0] == b'-' {
        (true, &bytes[1..])
    } else if bytes[0] == b'+' {
        (false, &bytes[1..])
    } else {
        (false, bytes)
    };

    let mut integer_part: u32 = 0;
    let mut fraction_part: u32 = 0;
    let mut fraction_digits: u32 = 0;
    let mut seen_dot = false;
    let mut any_digit = false;

    for &b in chars {
        if b == b'.' {
            if seen_dot {
                return None;
            }
            seen_dot = true;
        } else if b.is_ascii_digit() {
            any_digit = true;
            let d = (b - b'0') as u32;
            if seen_dot {
                if fraction_digits < 6 {
                    fraction_part = fraction_part * 10 + d;
                    fraction_digits += 1;
                }
            } else {
                integer_part = integer_part.checked_mul(10)?.checked_add(d)?;
            }
        } else {
            return None;
        }
    }

    if !any_digit {
        return None;
    }

    let mut result = integer_part as f32;
    if fraction_digits > 0 {
        let mut divisor = 1u32;
        for _ in 0..fraction_digits {
            divisor *= 10;
        }
        result += fraction_part as f32 / divisor as f32;
    }
    if negative {
        result = -result;
    }
    Some(result)
}

fn parse_f32_bytes(s: &[u8]) -> Option<f32> {
    core::str::from_utf8(s).ok().and_then(parse_f32)
}

fn line_contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.len() > haystack.len() {
        return false;
    }
    for i in 0..=(haystack.len() - needle.len()) {
        if &haystack[i..i + needle.len()] == needle {
            return true;
        }
    }
    false
}

fn extract_value_after(line: &[u8], prefix: &[u8]) -> Option<f32> {
    let plen = prefix.len();
    if plen > line.len() {
        return None;
    }
    let mut start = None;
    for i in 0..=(line.len() - plen) {
        if &line[i..i + plen] == prefix {
            start = Some(i + plen);
            break;
        }
    }
    let start = start?;
    let mut end = start;
    while end < line.len() && (line[end].is_ascii_digit() || line[end] == b'.') {
        end += 1;
    }
    if end == start {
        return None;
    }
    parse_f32_bytes(&line[start..end])
}

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
        config.rcc.hsi48 = Some(Hsi48Config { sync_from_usb: false });
    }
    let p = embassy_stm32::init(config);

    info!("Polar Pilot starting...");

    // Publish initial state
    STATE.sender().send(RotatorState::default());

    // ── LED heartbeat ───────────────────────────────────────────
    let led = Output::new(p.PB3, Level::High, Speed::Low);
    spawner.spawn(unwrap!(led_task(led)));

    // ── W5500 Ethernet ──────────────────────────────────────────
    let mut w5500_rst = Output::new(p.PA1, Level::Low, Speed::Low);
    Timer::after_millis(1).await;
    w5500_rst.set_high();
    Timer::after_millis(2).await;

    let mut spi_cfg = spi::Config::default();
    spi_cfg.frequency = Hertz(1_000_000);
    let spi = Spi::new(
        p.SPI1, p.PA5, p.PA7, p.PA6,
        p.DMA1_CH3, p.DMA1_CH2,
        Irqs, spi_cfg,
    );
    let cs = Output::new(p.PA4, Level::High, Speed::VeryHigh);
    let spi_dev = ExclusiveDevice::new(spi, cs, embassy_time::Delay).unwrap();

    let w5500_int = ExtiInput::new(p.PA3, p.EXTI3, Pull::Up, Irqs);
    let mac = [0x02, 0x00, 0x00, 0x00, 0x00, 0x01];

    static W5500_STATE: StaticCell<embassy_net_wiznet::State<2, 2>> = StaticCell::new();
    let w5500_state = W5500_STATE.init(embassy_net_wiznet::State::<2, 2>::new());

    let (device, w5500_runner) =
        embassy_net_wiznet::new(mac, w5500_state, spi_dev, w5500_int, w5500_rst).await.unwrap();

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

    // Wait for DHCP
    info!("Waiting for DHCP...");
    loop {
        if let Some(cfg) = stack.config_v4() {
            info!("DHCP: IP {}", cfg.address);
            break;
        }
        Timer::after_millis(500).await;
    }

    // ── OLED display (I2C1) ─────────────────────────────────────
    let i2c = I2c::new_blocking(p.I2C1, p.PB6, p.PB7, Default::default());
    spawner.spawn(unwrap!(display_task(i2c)));

    // ── Stepper motors ──────────────────────────────────────────
    let motor_en = Output::new(p.PB4, Level::High, Speed::Low);
    let az_dir = Output::new(p.PC14, Level::Low, Speed::Low);
    let el_dir = Output::new(p.PA9, Level::Low, Speed::Low);
    let az_home = Input::new(p.PA10, Pull::Up);
    let el_home = Input::new(p.PB5, Pull::Up);

    let el_step = PwmPin::new(p.PA0, OutputType::PushPull);
    let el_pwm = SimplePwm::new(
        p.TIM2, Some(el_step), None, None, None,
        Hertz(STEP_HZ), Default::default(),
    );

    let az_step = PwmPin::new(p.PA8, OutputType::PushPull);
    let az_pwm = SimplePwm::new(
        p.TIM1, Some(az_step), None, None, None,
        Hertz(STEP_HZ), Default::default(),
    );

    spawner.spawn(unwrap!(motor_task(az_pwm, el_pwm, az_dir, el_dir, motor_en, az_home, el_home)));

    // ── 5-way navigation buttons ────────────────────────────────
    let btn_up = Input::new(p.PB1, Pull::Up);
    let btn_down = Input::new(p.PB0, Pull::Up);
    let btn_left = Input::new(p.PA11, Pull::Up);
    let btn_right = Input::new(p.PA12, Pull::Up);
    let btn_center = Input::new(p.PC15, Pull::Up);
    spawner.spawn(unwrap!(key_task(btn_up, btn_down, btn_left, btn_right, btn_center)));

    // ── Rotctld TCP server ──────────────────────────────────────
    spawner.spawn(unwrap!(rotctld_task(stack)));

    // ── EasyComm II serial (USART2) ─────────────────────────────
    let mut usart_cfg = usart::Config::default();
    usart_cfg.baudrate = 9600;
    let uart = usart::Uart::new(
        p.USART2, p.PA15, p.PA2,
        p.DMA1_CH7, p.DMA1_CH6, Irqs, usart_cfg,
    ).unwrap();
    let (usart_tx, usart_rx) = uart.split();
    spawner.spawn(unwrap!(easycom_task(usart_rx, usart_tx)));

    info!("All tasks spawned");

    // Main task idles
    loop {
        Timer::after_secs(3600).await;
    }
}

// ── LED heartbeat task ──────────────────────────────────────────────

#[embassy_executor::task]
async fn led_task(mut led: Output<'static>) -> ! {
    let mut ticker = Ticker::every(Duration::from_secs(1));
    loop {
        ticker.next().await;
        led.toggle();
    }
}

// ── W5500 Ethernet driver task ──────────────────────────────────────

#[embassy_executor::task]
async fn ethernet_task(
    runner: embassy_net_wiznet::Runner<
        'static,
        embassy_net_wiznet::chip::W5500,
        ExclusiveDevice<Spi<'static, embassy_stm32::mode::Async, spi::mode::Master>, Output<'static>, embassy_time::Delay>,
        ExtiInput<'static, embassy_stm32::mode::Async>,
        Output<'static>,
    >,
) -> ! {
    runner.run().await
}

// ── Network stack task ──────────────────────────────────────────────

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, embassy_net_wiznet::Device<'static>>) -> ! {
    runner.run().await
}

// ── Motor control task ──────────────────────────────────────────────

#[embassy_executor::task]
async fn motor_task(
    mut az_pwm: SimplePwm<'static, embassy_stm32::peripherals::TIM1>,
    mut el_pwm: SimplePwm<'static, embassy_stm32::peripherals::TIM2>,
    mut az_dir: Output<'static>,
    mut el_dir: Output<'static>,
    mut motor_en: Output<'static>,
    az_home: Input<'static>,
    el_home: Input<'static>,
) -> ! {
    motor_en.set_high(); // disabled

    az_pwm.set_frequency(Hertz(STEP_HZ));
    let max_az = az_pwm.ch1().max_duty_cycle();
    az_pwm.ch1().set_duty_cycle(max_az / 2);
    az_pwm.ch1().disable();

    el_pwm.set_frequency(Hertz(STEP_HZ));
    let max_el = el_pwm.ch1().max_duty_cycle();
    el_pwm.ch1().set_duty_cycle(max_el / 2);
    el_pwm.ch1().disable();

    let mut target_az: f32 = 0.0;
    let mut target_el: f32 = 0.0;
    let mut current_az: f32 = 0.0;
    let mut current_el: f32 = 0.0;

    let state_tx = STATE.sender();
    info!("motor_task: running");

    loop {
        match embassy_time::with_timeout(MOTOR_TICK, CMD.receive()).await {
            Ok(cmd) => match cmd {
                RotatorCmd::GoTo { az, el } => {
                    info!("motor: GoTo az={} el={}", az, el);
                    target_az = az;
                    target_el = el;
                }
                RotatorCmd::Stop => {
                    info!("motor: Stop");
                    target_az = current_az;
                    target_el = current_el;
                }
            },
            Err(_) => {} // timeout — run periodic update
        }

        // Slew current position toward target
        let az_diff = target_az - current_az;
        let el_diff = target_el - current_el;
        let az_moving = az_diff.abs() > POS_EPSILON;
        let el_moving = el_diff.abs() > POS_EPSILON;

        if az_moving {
            if az_diff > 0.0 {
                az_dir.set_high();
                current_az += SLEW_RATE.min(az_diff);
            } else {
                az_dir.set_low();
                current_az -= SLEW_RATE.min(-az_diff);
            }
            az_pwm.ch1().enable();
        } else {
            current_az = target_az;
            az_pwm.ch1().disable();
        }

        if el_moving {
            if el_diff > 0.0 {
                el_dir.set_high();
                current_el += SLEW_RATE.min(el_diff);
            } else {
                el_dir.set_low();
                current_el -= SLEW_RATE.min(-el_diff);
            }
            el_pwm.ch1().enable();
        } else {
            current_el = target_el;
            el_pwm.ch1().disable();
        }

        // Endstop safety
        if az_home.is_low() && az_diff < 0.0 {
            current_az = 0.0;
            target_az = 0.0;
            az_pwm.ch1().disable();
        }
        if el_home.is_low() && el_diff < 0.0 {
            current_el = 0.0;
            target_el = 0.0;
            el_pwm.ch1().disable();
        }

        let moving = az_moving || el_moving;
        if moving {
            motor_en.set_low();
        } else {
            motor_en.set_high();
        }

        state_tx.send(RotatorState {
            target_az,
            target_el,
            current_az,
            current_el,
            moving,
            link_up: false,
        });
    }
}

// ── OLED display task ───────────────────────────────────────────────

#[embassy_executor::task]
async fn display_task(i2c: I2c<'static, embassy_stm32::mode::Blocking, embassy_stm32::i2c::Master>) -> ! {
    let interface = I2CDisplayInterface::new(i2c);
    let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
        .into_buffered_graphics_mode();

    loop {
        match display.init() {
            Ok(_) => {
                info!("OLED initialized");
                break;
            }
            Err(_) => {
                warn!("OLED init failed, retrying...");
                Timer::after_secs(1).await;
            }
        }
    }

    let thin_stroke = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
    let text_style = MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(BinaryColor::On)
        .build();

    const CX: i32 = 40;
    const CY: i32 = 32;
    const R_OUTER: i32 = 28;
    const R_INNER: i32 = 14;
    const PI: f32 = core::f32::consts::PI;
    const TX: i32 = 84;

    // Airplane pixel offsets (cross shape)
    const PLANE: [(i32, i32); 9] = [
        (0, -2), (0, -1),
        (-2, 0), (-1, 0), (0, 0), (1, 0), (2, 0),
        (0, 1), (0, 2),
    ];

    let mut ticker = Ticker::every(Duration::from_millis(250));
    let mut buf: heapless::String<16> = heapless::String::new();

    loop {
        ticker.next().await;

        let state = match STATE.try_get() {
            Some(s) => s,
            None => continue,
        };

        display.clear_buffer();

        // Outer ring
        let _ = Circle::with_center(Point::new(CX, CY), (R_OUTER * 2 + 1) as u32)
            .into_styled(thin_stroke)
            .draw(&mut display);

        // Inner ring
        let _ = Circle::with_center(Point::new(CX, CY), (R_INNER * 2 + 1) as u32)
            .into_styled(thin_stroke)
            .draw(&mut display);

        // Crosshair
        let _ = Line::new(Point::new(CX, CY - R_OUTER), Point::new(CX, CY + R_OUTER))
            .into_styled(thin_stroke)
            .draw(&mut display);
        let _ = Line::new(Point::new(CX - R_OUTER, CY), Point::new(CX + R_OUTER, CY))
            .into_styled(thin_stroke)
            .draw(&mut display);

        // Cardinal labels
        let _ = Text::new("N", Point::new(CX - 3, CY - R_OUTER - 2), text_style).draw(&mut display);
        let _ = Text::new("S", Point::new(CX - 3, CY + R_OUTER + 10), text_style).draw(&mut display);
        let _ = Text::new("E", Point::new(CX + R_OUTER + 2, CY + 4), text_style).draw(&mut display);
        let _ = Text::new("W", Point::new(CX - R_OUTER - 8, CY + 4), text_style).draw(&mut display);

        // Airplane position
        let angle_rad = state.current_az * PI / 180.0;
        let el_clamped = state.current_el.max(0.0).min(90.0);
        let r = (R_OUTER as f32) * (1.0 - el_clamped / 90.0);
        let px = CX as f32 + r * libm::sinf(angle_rad);
        let py = CY as f32 - r * libm::cosf(angle_rad);
        let px_i = px as i32;
        let py_i = py as i32;

        for &(dx, dy) in &PLANE {
            let _ = Pixel(Point::new(px_i + dx, py_i + dy), BinaryColor::On)
                .draw(&mut display);
        }

        // Status text
        buf.clear();
        let _ = core::write!(buf, "AZ:{:5.1}", state.current_az);
        let _ = Text::new(&buf, Point::new(TX, 10), text_style).draw(&mut display);

        buf.clear();
        let _ = core::write!(buf, "EL:{:5.1}", state.current_el);
        let _ = Text::new(&buf, Point::new(TX, 22), text_style).draw(&mut display);

        buf.clear();
        let _ = core::write!(buf, ">{:5.1}", state.target_az);
        let _ = Text::new(&buf, Point::new(TX, 36), text_style).draw(&mut display);

        buf.clear();
        let _ = core::write!(buf, ">{:5.1}", state.target_el);
        let _ = Text::new(&buf, Point::new(TX, 48), text_style).draw(&mut display);

        let net_str = if state.link_up { "NET" } else { "---" };
        let mov_str = if state.moving { " GO" } else { "   " };
        buf.clear();
        let _ = core::write!(buf, "{}{}", net_str, mov_str);
        let _ = Text::new(&buf, Point::new(TX, 62), text_style).draw(&mut display);

        if display.flush().is_err() {
            warn!("OLED flush failed");
        }
    }
}

// ── 5-way navigation key task ───────────────────────────────────────

#[embassy_executor::task]
async fn key_task(
    btn_up: Input<'static>,
    btn_down: Input<'static>,
    btn_left: Input<'static>,
    btn_right: Input<'static>,
    btn_center: Input<'static>,
) -> ! {
    const DEBOUNCE_SAMPLES: u8 = 2;
    const HOLD_TICKS: u16 = 25;   // 500 ms
    const REPEAT_TICKS: u16 = 10; // 200 ms

    struct BtnState {
        raw_count: u8,
        pressed: bool,
        hold_counter: u16,
    }

    impl BtnState {
        const fn new() -> Self {
            Self { raw_count: 0, pressed: false, hold_counter: 0 }
        }

        fn update(&mut self, pin_low: bool) -> bool {
            if pin_low {
                if self.raw_count < DEBOUNCE_SAMPLES {
                    self.raw_count += 1;
                }
            } else {
                self.raw_count = 0;
            }

            let now_pressed = self.raw_count >= DEBOUNCE_SAMPLES;

            if now_pressed && !self.pressed {
                self.pressed = true;
                self.hold_counter = 0;
                return true;
            }
            if !now_pressed && self.pressed {
                self.pressed = false;
                self.hold_counter = 0;
                return false;
            }
            if self.pressed {
                self.hold_counter = self.hold_counter.saturating_add(1);
                if self.hold_counter >= HOLD_TICKS
                    && (self.hold_counter - HOLD_TICKS) % REPEAT_TICKS == 0
                {
                    return true;
                }
            }
            false
        }
    }

    let mut up = BtnState::new();
    let mut down = BtnState::new();
    let mut left = BtnState::new();
    let mut right = BtnState::new();
    let mut center = BtnState::new();

    let mut ticker = Ticker::every(Duration::from_millis(20));

    loop {
        ticker.next().await;

        let fire_up = up.update(btn_up.is_low());
        let fire_down = down.update(btn_down.is_low());
        let fire_left = left.update(btn_left.is_low());
        let fire_right = right.update(btn_right.is_low());
        let fire_center = center.update(btn_center.is_low());

        if fire_center {
            CMD.send(RotatorCmd::Stop).await;
            continue;
        }

        if fire_up || fire_down || fire_left || fire_right {
            let state = STATE.try_get().unwrap_or_default();
            let mut az = state.target_az;
            let mut el = state.target_el;

            if fire_right { az += 1.0; }
            if fire_left { az -= 1.0; }
            if fire_up { el += 1.0; }
            if fire_down { el -= 1.0; }

            if az >= 360.0 { az -= 360.0; }
            if az < 0.0 { az += 360.0; }
            el = el.max(0.0).min(90.0);

            CMD.send(RotatorCmd::GoTo { az, el }).await;
        }
    }
}

// ── Rotctld TCP server task ─────────────────────────────────────────

#[embassy_executor::task]
async fn rotctld_task(stack: embassy_net::Stack<'static>) -> ! {
    let mut rx_buf = [0u8; 256];
    let mut tx_buf = [0u8; 256];

    loop {
        let mut socket = TcpSocket::new(stack, &mut rx_buf, &mut tx_buf);
        socket.set_timeout(Some(Duration::from_secs(30)));

        if socket.accept(4533).await.is_err() {
            Timer::after_millis(500).await;
            continue;
        }

        info!("rotctld: client connected");

        let mut line_buf = [0u8; 128];
        let mut line_len: usize = 0;

        'conn: loop {
            let mut byte = [0u8; 1];
            match socket.read(&mut byte).await {
                Ok(0) | Err(_) => break 'conn,
                Ok(_) => {}
            }

            if byte[0] == b'\n' {
                let end = if line_len > 0 && line_buf[line_len - 1] == b'\r' {
                    line_len - 1
                } else {
                    line_len
                };
                let line = core::str::from_utf8(&line_buf[..end]).unwrap_or("");
                let mut resp: heapless::String<256> = heapless::String::new();

                if line == "p" || line == "\\get_pos" {
                    let state = STATE.try_get().unwrap_or_default();
                    let _ = core::write!(resp, "{:.1}\n{:.1}\n", state.current_az, state.current_el);
                } else if line.starts_with("P ") || line.starts_with("\\set_pos ") {
                    let args = if line.starts_with("P ") { &line[2..] } else { &line[9..] };
                    let mut parts = args.splitn(2, ' ');
                    let az = parts.next().and_then(parse_f32);
                    let el = parts.next().and_then(parse_f32);
                    match (az, el) {
                        (Some(az), Some(el)) => {
                            CMD.send(RotatorCmd::GoTo { az, el }).await;
                            let _ = core::write!(resp, "RPRT 0\n");
                        }
                        _ => {
                            let _ = core::write!(resp, "RPRT -1\n");
                        }
                    }
                } else if line == "S" || line == "\\stop" {
                    CMD.send(RotatorCmd::Stop).await;
                    let _ = core::write!(resp, "RPRT 0\n");
                } else if line == "q" || line == "\\quit" {
                    break 'conn;
                } else if line == "_" || line == "\\get_info" {
                    let _ = core::write!(resp, "Model: Polar Pilot\n");
                } else if line == "\\dump_state" {
                    let _ = core::write!(resp, "0\nrot_model=0\nmin_az=0.0\nmax_az=360.0\nmin_el=0.0\nmax_el=90.0\n0\n0\n");
                } else {
                    let _ = core::write!(resp, "RPRT -1\n");
                }

                if !resp.is_empty() {
                    if socket.write_all(resp.as_bytes()).await.is_err() {
                        break 'conn;
                    }
                }
                line_len = 0;
            } else if line_len < line_buf.len() {
                line_buf[line_len] = byte[0];
                line_len += 1;
            } else {
                break 'conn;
            }
        }

        info!("rotctld: client disconnected");
        socket.close();
        Timer::after_millis(100).await;
    }
}

// ── EasyComm II serial task ─────────────────────────────────────────

#[embassy_executor::task]
async fn easycom_task(
    mut usart_rx: usart::UartRx<'static, embassy_stm32::mode::Async>,
    mut usart_tx: usart::UartTx<'static, embassy_stm32::mode::Async>,
) -> ! {
    info!("EasyComm II task started");

    let mut buf = [0u8; 128];
    let mut pos: usize = 0;

    loop {
        let mut tmp = [0u8; 64];
        let n = match usart_rx.read_until_idle(&mut tmp).await {
            Ok(n) => n,
            Err(_) => {
                warn!("USART read error");
                pos = 0;
                continue;
            }
        };

        for i in 0..n {
            let b = tmp[i];

            if b == b'\n' || b == b'\r' {
                if pos == 0 {
                    continue;
                }

                let line = &buf[..pos];
                pos = 0;

                if line == b"VE" {
                    let _ = usart_tx.write(b"Polar Pilot v0.1\n").await;
                } else if line == b"SA" || line == b"SE" || line == b"SA SE" {
                    CMD.send(RotatorCmd::Stop).await;
                } else {
                    let has_az = line.starts_with(b"AZ");
                    let has_el = line_contains(line, b"EL");

                    if has_az || has_el {
                        let az_val = extract_value_after(line, b"AZ");
                        let el_val = extract_value_after(line, b"EL");

                        if az_val.is_none() && el_val.is_none() && has_az {
                            let state = STATE.try_get().unwrap_or_default();
                            let mut resp = heapless::String::<40>::new();
                            let _ = core::write!(
                                resp, "AZ{:.1} EL{:.1}\n",
                                state.current_az, state.current_el,
                            );
                            let _ = usart_tx.write(resp.as_bytes()).await;
                        } else {
                            let state = STATE.try_get().unwrap_or_default();
                            let az = az_val.unwrap_or(state.target_az);
                            let el = el_val.unwrap_or(state.target_el);
                            CMD.send(RotatorCmd::GoTo { az, el }).await;
                        }
                    } else {
                        warn!("EasyComm: unknown cmd");
                    }
                }
            } else if pos < buf.len() {
                buf[pos] = b;
                pos += 1;
            } else {
                warn!("EasyComm: line overflow");
                pos = 0;
            }
        }
    }
}
