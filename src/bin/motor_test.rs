//! Motor test — drives AZ and EL steppers via hardware PWM, toggling
//! direction every 2 seconds.
//!
//! Pins:
//!   AZ STEP = PA8  (TIM1_CH1)    AZ DIR = PC14
//!   EL STEP = PA0  (TIM2_CH1)    EL DIR = PA9
//!   Motor EN = PB4 (active-low, shared enable for both drivers)
//!   Heartbeat = PB3 (onboard LED, 1 Hz)
//!
//! Hook a scope to PA0 and PA8 to see the step pulses.
//!
//! Run with:  cargo run --release --bin motor_test

#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, OutputType, Speed};
use embassy_stm32::time::Hertz;
use embassy_stm32::timer::simple_pwm::{PwmPin, SimplePwm};
use embassy_time::{Duration, Ticker, Timer};
use {defmt_rtt as _, panic_reset as _};

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
    }
    let p = embassy_stm32::init(config);
    let led = Output::new(p.PB3, Level::High, Speed::Low);

    info!("Motor test starting...");

    // ── Motor enable (active-low) — assert before starting PWM ────
    let _motor_en = Output::new(p.PB4, Level::Low, Speed::Low);
    info!("Motor EN asserted (PB4 low)");

    // ── Direction pins ────────────────────────────────────────────
    let mut az_dir = Output::new(p.PC14, Level::Low, Speed::Low);
    let mut el_dir = Output::new(p.PA9, Level::Low, Speed::Low);

    // ── EL STEP — TIM2_CH1 on PA0 (1000 Hz) ──────────────────────
    let el_step = PwmPin::new(p.PA0, OutputType::PushPull);
    let mut el_pwm = SimplePwm::new(
        p.TIM2,
        Some(el_step),
        None,
        None,
        None,
        Hertz(1_000),
        Default::default(),
    );
    {
        let mut ch = el_pwm.ch1();
        ch.set_duty_cycle(ch.max_duty_cycle() / 2);
        ch.enable();
    }
    info!("EL STEP: PA0, TIM2_CH1, 1000 Hz");

    // ── AZ STEP — TIM1_CH1 on PA8 (500 Hz) ───────────────────────
    let az_step = PwmPin::new(p.PA8, OutputType::PushPull);
    let mut az_pwm = SimplePwm::new(
        p.TIM1,
        Some(az_step),
        None,
        None,
        None,
        Hertz(500),
        Default::default(),
    );
    {
        let mut ch = az_pwm.ch1();
        ch.set_duty_cycle(ch.max_duty_cycle() / 2);
        ch.enable();
    }
    info!("AZ STEP: PA8, TIM1_CH1, 500 Hz");

    // ── Heartbeat on separate task ────────────────────────────────
    spawner.spawn(unwrap!(led_task(led)));

    // ── Toggle direction every 2 s ────────────────────────────────
    let mut forward = true;
    loop {
        Timer::after_secs(2).await;
        forward = !forward;
        if forward {
            az_dir.set_low();
            el_dir.set_low();
            info!("Direction: forward");
        } else {
            az_dir.set_high();
            el_dir.set_high();
            info!("Direction: reverse");
        }
    }
}

#[embassy_executor::task]
async fn led_task(mut led: Output<'static>) -> ! {
    let mut ticker = Ticker::every(Duration::from_secs(1));
    loop {
        ticker.next().await;
        led.toggle();
    }
}
