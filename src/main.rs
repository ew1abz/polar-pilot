//! Minimal step signal generator — TIM2_CH1 (PA0) + TIM1_CH1 (PA8)
//!
//! The timer output-compare hardware drives the pin directly —
//! no ISR needed to toggle STEP.  You control speed by changing
//! the timer's frequency, and duty cycle sets the pulse width.
//!
//! Hook a scope to PA0 and PA8 to see the pulses.
//! Direction pins (PC14, PA9) toggle every 2 seconds for visual confirmation.

#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, OutputType, Speed};
use embassy_stm32::time::Hertz;
use embassy_stm32::timer::simple_pwm::{PwmPin, SimplePwm};
use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) -> ! {
    // ── Clock config: 80 MHz from HSI+PLL (same as before) ──────
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

    info!("Step signal generator starting...");

    // ── Direction pins (active-low/high depends on driver board) ─
    let mut az_dir = Output::new(p.PC14, Level::Low, Speed::Low);
    let mut el_dir = Output::new(p.PA9, Level::Low, Speed::Low);

    // ── AZ STEP — TIM2 channel 1 on PA0 ─────────────────────────
    //
    // SimplePwm configures the timer in edge-aligned PWM mode:
    //   - ARR (auto-reload) sets the period  → controls step frequency
    //   - CCR1 (compare value) sets the pulse width → controls duty cycle
    //   - The hardware toggles PA0 automatically, zero CPU involvement
    //
    // Frequency = steps per second.  For a 200-step motor with 16x
    // microstepping, 1000 Hz ≈ 0.3 RPM — nice and slow for testing.
    let az_step = PwmPin::new(p.PA0, OutputType::PushPull);
    let mut az_pwm = SimplePwm::new(
        p.TIM2,
        Some(az_step),
        None, // CH2 unused
        None, // CH3 unused
        None, // CH4 unused
        Hertz(1_000),
        Default::default(), // edge-aligned up-counting
    );

    // Duty cycle: 50% makes a clean square wave, easy to see on a scope.
    // For real stepper drivers (DRV8825) you'd use a narrow pulse (~5 µs),
    // but for this demo symmetric is clearer.
    {
        let mut ch = az_pwm.ch1();
        ch.set_duty_cycle(ch.max_duty_cycle() / 2);
        ch.enable();
    }

    info!("AZ STEP: PA0, TIM2_CH1, 1000 Hz");

    // ── EL STEP — TIM1 channel 1 on PA8 ─────────────────────────
    //
    // TIM1 is an "advanced" timer (has break/deadtime features for
    // motor control).  For simple PWM it works identically to TIM2.
    let el_step = PwmPin::new(p.PA8, OutputType::PushPull);
    let mut el_pwm = SimplePwm::new(
        p.TIM1,
        Some(el_step),
        None,
        None,
        None,
        Hertz(500),
        Default::default(),
    );

    {
        let mut ch = el_pwm.ch1();
        ch.set_duty_cycle(ch.max_duty_cycle() / 2);
        ch.enable();
    }

    info!("EL STEP: PA8, TIM1_CH1, 500 Hz");

    // ── Demo loop: toggle direction every 2 s ────────────────────
    //
    // To change speed at runtime:
    //   az_pwm.set_frequency(Hertz(new_freq));
    //   az_pwm.ch1().set_duty_cycle(az_pwm.ch1().max_duty_cycle() / 2);
    //
    // To stop:
    //   az_pwm.ch1().disable();
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
