//! Button test — polls the 5-way navigation switch and logs state via RTT.
//!
//! Pins (active-low, internal pull-up):
//!   UP=PB1  DOWN=PB0  LEFT=PA11  RIGHT=PA12  CENTER=PC15
//!
//! Run with:  cargo run --release --bin button_test

#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Input, Level, Output, Pull, Speed};
use embassy_time::{Duration, Ticker};
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) -> ! {
    let p = embassy_stm32::init(Default::default());

    let mut led = Output::new(p.PB3, Level::High, Speed::Low);

    let btn_up = Input::new(p.PB1, Pull::Up);
    let btn_down = Input::new(p.PB0, Pull::Up);
    let btn_left = Input::new(p.PA11, Pull::Up);
    let btn_right = Input::new(p.PA12, Pull::Up);
    let btn_center = Input::new(p.PC15, Pull::Up);

    info!("Button test ready — press any button");
    info!("Pins: UP=PB1 DOWN=PB0 LEFT=PA11 RIGHT=PA12 CENTER=PC15");

    let mut prev = [true; 5]; // all released (high) initially
    let mut ticker = Ticker::every(Duration::from_millis(20));

    loop {
        ticker.next().await;

        let cur = [
            btn_up.is_high(),
            btn_down.is_high(),
            btn_left.is_high(),
            btn_right.is_high(),
            btn_center.is_high(),
        ];

        let names = ["UP", "DOWN", "LEFT", "RIGHT", "CENTER"];

        for i in 0..5 {
            if cur[i] != prev[i] {
                if !cur[i] {
                    info!("{} pressed", names[i]);
                    led.set_low();
                } else {
                    info!("{} released", names[i]);
                    led.set_high();
                }
            }
        }

        prev = cur;
    }
}
