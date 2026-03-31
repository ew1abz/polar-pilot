use embassy_stm32::gpio::Input;
use embassy_time::{Duration, Ticker};

use crate::types::{Phase, RotatorCmd, CMD, STATE};

#[embassy_executor::task]
pub async fn key_task(
    btn_up: Input<'static>,
    btn_down: Input<'static>,
    btn_left: Input<'static>,
    btn_right: Input<'static>,
    btn_center: Input<'static>,
) -> ! {
    const DEBOUNCE_SAMPLES: u8 = 2;

    struct BtnState {
        raw_count: u8,
        pub pressed: bool,
    }

    impl BtnState {
        const fn new() -> Self {
            Self { raw_count: 0, pressed: false }
        }

        // Returns true on a press or release edge.
        fn update(&mut self, pin_low: bool) -> bool {
            if pin_low {
                if self.raw_count < DEBOUNCE_SAMPLES {
                    self.raw_count += 1;
                }
            } else {
                self.raw_count = 0;
            }
            let now = self.raw_count >= DEBOUNCE_SAMPLES;
            let edge = now != self.pressed;
            self.pressed = now;
            edge
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

        // Capture jogging state before updating debounce
        let was_jogging = right.pressed || left.pressed || up.pressed || down.pressed;

        right.update(btn_right.is_low());
        left.update(btn_left.is_low());
        up.update(btn_up.is_low());
        down.update(btn_down.is_low());
        let center_edge = center.update(btn_center.is_low()) && center.pressed;

        // Ignore all input during homing
        let state = STATE.try_get().unwrap_or_default();
        if state.phase != Phase::Running {
            continue;
        }

        // CENTER: park both axes to 0°/0°
        if center_edge {
            CMD.send(RotatorCmd::GoTo { az: 0.0, el: 0.0 }).await;
            continue;
        }

        // UP/DOWN jog AZ; LEFT/RIGHT jog EL.
        // Send a large fixed sentinel target; motor_task enforces the software
        // position limits (0–360° AZ, 0–90° EL) and stops the motor there.
        let jogging = up.pressed || down.pressed || right.pressed || left.pressed;

        if jogging {
            let az = if up.pressed        {  9999.0 }
                     else if down.pressed { -9999.0 }
                     else                 { state.current_az };
            let el = if right.pressed     {  9999.0 }
                     else if left.pressed { -9999.0 }
                     else                 { state.current_el };
            CMD.send(RotatorCmd::GoTo { az, el }).await;
        } else if was_jogging {
            CMD.send(RotatorCmd::Stop).await;
        }
    }
}
