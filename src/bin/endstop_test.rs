//! Endstop test — drives both steppers toward home until endstops trigger.
//!
//! Runs AZ and EL motors at low speed (200 Hz) in the "home" direction.
//! When an endstop goes low (active-low, internal pull-up), that axis
//! stops immediately. Once both axes are homed, LED blinks fast.
//!
//! Pins:
//!   AZ STEP = PA8  (TIM1_CH1)    AZ DIR = PC14
//!   EL STEP = PA0  (TIM2_CH1)    EL DIR = PA9
//!   Motor EN = PB4 (active-low)
//!   AZ Home = PA10 (active-low, pull-up)
//!   EL Home = PB5  (active-low, pull-up)
//!   Heartbeat = PB3 (onboard LED)
//!
//! Run with:  cargo run --release --bin endstop_test

#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Input, Level, Output, OutputType, Pull, Speed};
use embassy_stm32::time::Hertz;
use embassy_stm32::timer::simple_pwm::{PwmPin, SimplePwm};
use embassy_time::{Duration, Ticker, Timer};
use {defmt_rtt as _, panic_reset as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) -> ! {
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

    let mut led = Output::new(p.PB3, Level::High, Speed::Low);

    // ── Endstop inputs (active-low, internal pull-up) ───────────
    let az_home = Input::new(p.PA10, Pull::Up);
    let el_home = Input::new(p.PB5, Pull::Up);

    info!("Endstop test starting...");
    info!("AZ Home=PA10, EL Home=PB5 (active-low)");

    // ── Let filtering caps charge through pull-ups ──────────────
    Timer::after(Duration::from_millis(50)).await;

    // ── Check if already at home ────────────────────────────────
    let mut az_homed = az_home.is_low();
    let mut el_homed = el_home.is_low();

    if az_homed {
        info!("AZ endstop already triggered at startup");
    }
    if el_homed {
        info!("EL endstop already triggered at startup");
    }

    if az_homed && el_homed {
        info!("Both endstops triggered — already home!");
        blink_fast(&mut led).await;
    }

    // ── Motor enable (active-low) ───────────────────────────────
    let mut motor_en = Output::new(p.PB4, Level::Low, Speed::Low);
    info!("Motor EN asserted (PB4 low)");

    // ── Direction pins — set toward home ────────────────────────
    // Convention: LOW = toward home. Adjust if your wiring differs.
    let _az_dir = Output::new(p.PC14, Level::Low, Speed::Low);
    let _el_dir = Output::new(p.PA9, Level::Low, Speed::Low);
    info!("Direction: toward home (both LOW)");

    // ── EL STEP — TIM2_CH1 on PA0 (200 Hz) ─────────────────────
    let mut el_pwm = if !el_homed {
        let el_step = PwmPin::new(p.PA0, OutputType::PushPull);
        let mut pwm =
            SimplePwm::new(p.TIM2, Some(el_step), None, None, None, Hertz(200), Default::default());
        {
            let mut ch = pwm.ch1();
            ch.set_duty_cycle(ch.max_duty_cycle() / 2);
            ch.enable();
        }
        info!("EL stepping at 200 Hz");
        Some(pwm)
    } else {
        None
    };

    // ── AZ STEP — TIM1_CH1 on PA8 (200 Hz) ─────────────────────
    let mut az_pwm = if !az_homed {
        let az_step = PwmPin::new(p.PA8, OutputType::PushPull);
        let mut pwm =
            SimplePwm::new(p.TIM1, Some(az_step), None, None, None, Hertz(200), Default::default());
        {
            let mut ch = pwm.ch1();
            ch.set_duty_cycle(ch.max_duty_cycle() / 2);
            ch.enable();
        }
        info!("AZ stepping at 200 Hz");
        Some(pwm)
    } else {
        None
    };

    // ── Poll endstops at 10 ms ──────────────────────────────────
    let mut ticker = Ticker::every(Duration::from_millis(10));

    loop {
        ticker.next().await;

        if !az_homed && az_home.is_low() {
            az_homed = true;
            if let Some(ref mut pwm) = az_pwm {
                pwm.ch1().disable();
            }
            info!("AZ endstop triggered — AZ stopped");
            led.toggle();
        }

        if !el_homed && el_home.is_low() {
            el_homed = true;
            if let Some(ref mut pwm) = el_pwm {
                pwm.ch1().disable();
            }
            info!("EL endstop triggered — EL stopped");
            led.toggle();
        }

        if az_homed && el_homed {
            info!("Both axes homed!");
            motor_en.set_high();
            blink_fast(&mut led).await;
        }
    }
}

async fn blink_fast(led: &mut Output<'_>) -> ! {
    info!("Homing complete — blinking fast");
    loop {
        led.toggle();
        Timer::after(Duration::from_millis(100)).await;
    }
}
