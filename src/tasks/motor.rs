use defmt::*;
use embassy_stm32::gpio::{Input, Output};
use embassy_stm32::time::Hertz;
use embassy_stm32::timer::simple_pwm::SimplePwm;
use embassy_time::{with_timeout, Duration, Instant, Timer};

use crate::types::{Phase, RotatorCmd, RotatorState, CMD, LIMITS, STATE};

pub const STEP_HZ: u32 = 4_000;

const STEPS_PER_REV: f32 = 200.0;
const GEAR_RATIO: f32 = 54.0;
const AZ_MICROSTEPS: f32 = 32.0;
const EL_MICROSTEPS: f32 = 32.0; // EL driver MS pins wired for 16; change to 32 if matched
const MOTOR_TICK_MS: u64 = 1;

// Degrees per physical step pulse.
const AZ_DEG_PER_STEP: f32 = 360.0 / (STEPS_PER_REV * AZ_MICROSTEPS * GEAR_RATIO);
const EL_DEG_PER_STEP: f32 = 360.0 / (STEPS_PER_REV * EL_MICROSTEPS * GEAR_RATIO);
// Steps issued per microsecond by the hardware timer.
const STEPS_PER_US: f32 = STEP_HZ as f32 / 1_000_000.0;

const MOTOR_TICK: Duration = Duration::from_millis(MOTOR_TICK_MS);
const POS_EPSILON: f32 = 0.05;

#[embassy_executor::task]
pub async fn motor_task(
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

    let state_tx = STATE.sender();
    info!("motor_task: running");

    // Homing constants
    // Back-off: drive positive until endstop releases (or 3 s timeout).
    const BACKOFF_TIMEOUT: Duration = Duration::from_secs(3);
    // Maximum approach travel before declaring a fault.
    const MAX_AZ_HOME_MS: u64 = 95_000; // 380° ÷ ~4°/s
    const MAX_EL_HOME_MS: u64 = 28_000; // 100° ÷ ~4°/s

    // Outer loop: re-entered each time a Home command is received.
    loop {
        // Drain any pending commands so stale GoTo/Stop don't interfere.
        while CMD.try_receive().is_ok() {}

        motor_en.set_low();

        let mut target_az: f32 = 0.0;
        let mut target_el: f32 = 0.0;
        let mut current_az: f32 = 0.0;
        let mut current_el: f32 = 0.0;

        // ── AZ homing ───────────────────────────────────────────────
        info!("motor: homing AZ");

        // Pre-check: if already on endstop, back off until it releases.
        if az_home.is_low() {
            info!("motor: AZ on endstop, backing off");
            az_dir.set_high();
            az_pwm.ch1().enable();
            let result = with_timeout(BACKOFF_TIMEOUT, async {
                loop {
                    if az_home.is_high() {
                        return;
                    }
                    Timer::after(MOTOR_TICK).await;
                }
            })
            .await;
            az_pwm.ch1().disable();
            if result.is_err() || az_home.is_low() {
                motor_en.set_high();
                error!("motor: AZ endstop stuck -- halting");
                state_tx.send(RotatorState {
                    phase: Phase::Fault("AZ endstop stuck"),
                    ..Default::default()
                });
                loop {
                    Timer::after_secs(60).await;
                }
            }
        }

        // Approach home.
        az_dir.set_low();
        az_pwm.ch1().enable();
        let deadline = Instant::now() + Duration::from_millis(MAX_AZ_HOME_MS);
        loop {
            if az_home.is_low() {
                az_pwm.ch1().disable();
                current_az = 0.0;
                target_az = 0.0;
                info!("motor: AZ homed");
                break;
            }
            if Instant::now() > deadline {
                az_pwm.ch1().disable();
                motor_en.set_high();
                error!("motor: AZ travel limit exceeded -- halting");
                state_tx.send(RotatorState {
                    phase: Phase::Fault("AZ travel limit"),
                    ..Default::default()
                });
                loop {
                    Timer::after_secs(60).await;
                }
            }
            state_tx.send(RotatorState {
                target_az,
                target_el,
                current_az,
                current_el,
                moving: true,
                link_up: false,
                phase: Phase::Homing,
            });
            Timer::after(MOTOR_TICK).await;
        }

        // ── EL homing ───────────────────────────────────────────────
        info!("motor: homing EL");

        if el_home.is_low() {
            info!("motor: EL on endstop, backing off");
            el_dir.set_high();
            el_pwm.ch1().enable();
            let result = with_timeout(BACKOFF_TIMEOUT, async {
                loop {
                    if el_home.is_high() {
                        return;
                    }
                    Timer::after(MOTOR_TICK).await;
                }
            })
            .await;
            el_pwm.ch1().disable();
            if result.is_err() || el_home.is_low() {
                motor_en.set_high();
                error!("motor: EL endstop stuck -- halting");
                state_tx.send(RotatorState {
                    phase: Phase::Fault("EL endstop stuck"),
                    ..Default::default()
                });
                loop {
                    Timer::after_secs(60).await;
                }
            }
        }

        el_dir.set_low();
        el_pwm.ch1().enable();
        let deadline = Instant::now() + Duration::from_millis(MAX_EL_HOME_MS);
        loop {
            if el_home.is_low() {
                el_pwm.ch1().disable();
                current_el = 0.0;
                target_el = 0.0;
                info!("motor: EL homed");
                break;
            }
            if Instant::now() > deadline {
                el_pwm.ch1().disable();
                motor_en.set_high();
                error!("motor: EL travel limit exceeded -- halting");
                state_tx.send(RotatorState {
                    phase: Phase::Fault("EL travel limit"),
                    ..Default::default()
                });
                loop {
                    Timer::after_secs(60).await;
                }
            }
            state_tx.send(RotatorState {
                target_az,
                target_el,
                current_az,
                current_el,
                moving: true,
                link_up: false,
                phase: Phase::Homing,
            });
            Timer::after(MOTOR_TICK).await;
        }

        motor_en.set_high();
        info!("motor: homing done");

        let mut last_tick = Instant::now();
        // Sub-step fractional accumulators: carry forward the unissued fraction of a
        // step between loop iterations so no pulses are lost to truncation.
        let mut az_frac: f32 = 0.0;
        let mut el_frac: f32 = 0.0;

        'running: loop {
            match embassy_time::with_timeout(MOTOR_TICK, CMD.receive()).await {
                Ok(cmd) => match cmd {
                    RotatorCmd::GoTo { az, el } => {
                        let lim = LIMITS.lock(|c| c.get());
                        target_az = az.clamp(lim.az_min, lim.az_max);
                        target_el = el.clamp(lim.el_min, lim.el_max);
                        info!(
                            "motor: GoTo az={} el={} (clamped to az={} el={})",
                            az, el, target_az, target_el
                        );
                    }
                    RotatorCmd::Park => {
                        target_az = 0.0;
                        target_el = 0.0;
                        info!("motor: Park (bypassing soft limits)");
                    }
                    RotatorCmd::Stop => {
                        info!("motor: Stop");
                        target_az = current_az;
                        target_el = current_el;
                    }
                    RotatorCmd::Home => {
                        info!("motor: re-homing requested");
                        az_pwm.ch1().disable();
                        el_pwm.ch1().disable();
                        motor_en.set_high();
                        break 'running;
                    }
                },
                Err(_) => {} // timeout — run periodic update
            }

            // Count whole steps the hardware timer issued since the last iteration.
            // Microsecond resolution prevents the integer-ms truncation error of as_millis().
            let now = Instant::now();
            let elapsed_us = (now - last_tick).as_micros() as f32;
            last_tick = now;

            let az_diff = target_az - current_az;
            let el_diff = target_el - current_el;
            let az_moving = az_diff.abs() > POS_EPSILON;
            let el_moving = el_diff.abs() > POS_EPSILON;

            if az_moving {
                az_frac += elapsed_us * STEPS_PER_US;
                let steps = az_frac as u32;
                az_frac -= steps as f32;
                let advance = steps as f32 * AZ_DEG_PER_STEP;
                if az_diff > 0.0 {
                    az_dir.set_high();
                    current_az = (current_az + advance).min(target_az);
                } else {
                    az_dir.set_low();
                    current_az = (current_az - advance).max(target_az);
                }
                az_pwm.ch1().enable();
            } else {
                current_az = target_az;
                az_frac = 0.0;
                az_pwm.ch1().disable();
            }

            if el_moving {
                el_frac += elapsed_us * STEPS_PER_US;
                let steps = el_frac as u32;
                el_frac -= steps as f32;
                let advance = steps as f32 * EL_DEG_PER_STEP;
                if el_diff > 0.0 {
                    el_dir.set_high();
                    current_el = (current_el + advance).min(target_el);
                } else {
                    el_dir.set_low();
                    current_el = (current_el - advance).max(target_el);
                }
                el_pwm.ch1().enable();
            } else {
                current_el = target_el;
                el_frac = 0.0;
                el_pwm.ch1().disable();
            }

            // Endstop safety (hardware)
            // if az_home.is_low() && az_diff < 0.0 {
            //     current_az = 0.0;
            //     target_az = 0.0;
            //     az_pwm.ch1().disable();
            // }
            if el_home.is_low() && el_diff < 0.0 {
                current_el = 0.0;
                target_el = 0.0;
                el_pwm.ch1().disable();
            }

            // Software position limits.
            // AZ lower limit removed — gpredict tracks satellites through north (wraps below 0°).
            // EL lower limit: hardware endstop is the physical safety, clamp prevents negative display.
            // Upper limits: no hardware endstop, so also kill PWM and reset target so
            //   el_moving / az_moving drops to false and the motor actually stops.
            if current_el <= 0.0 {
                current_el = 0.0;
            }
            if current_az >= 360.0 {
                current_az = 360.0;
                target_az = 360.0;
                az_pwm.ch1().disable();
            }
            if current_el >= 180.0 {
                current_el = 180.0;
                target_el = 180.0;
                el_pwm.ch1().disable();
            }

            let moving = az_moving || el_moving;
            if moving {
                motor_en.set_low();
            } else {
                motor_en.set_high();
            }

            // Clamp targets in published state so display never shows jog sentinels.
            state_tx.send(RotatorState {
                target_az: target_az.clamp(0.0, 360.0),
                target_el: target_el.clamp(0.0, 180.0),
                current_az,
                current_el,
                moving,
                link_up: false,
                phase: Phase::Running,
            });
        } // end 'running loop
    } // end outer homing loop
}
