//! OLED test — displays a counter on the SSD1306 128x64 display,
//! incrementing every second.
//!
//! Pins: I2C1 — PB6/SCL, PB7/SDA (address 0x3C)
//!
//! Run with:  cargo run --release --bin oled_test

#![no_std]
#![no_main]

use core::fmt::Write as FmtWrite;

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::i2c::I2c;
use embassy_time::{Duration, Ticker, Timer};
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::mono_font::MonoTextStyleBuilder;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Circle, PrimitiveStyle};
use embedded_graphics::text::Text;
use ssd1306::prelude::*;
use ssd1306::rotation::DisplayRotation;
use ssd1306::size::DisplaySize128x64;
use ssd1306::I2CDisplayInterface;
use ssd1306::Ssd1306;
use {defmt_rtt as _, panic_probe as _};

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

    info!("OLED test starting...");

    // ── I2C1 ──────────────────────────────────────────────────────
    // External pull-ups are fitted — do NOT enable internal pull-ups,
    // they stack in parallel and can cause SDA to sit at ~2 V.
    let i2c = I2c::new_blocking(p.I2C1, p.PB6, p.PB7, Default::default());

    let interface = I2CDisplayInterface::new(i2c);
    let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
        .into_buffered_graphics_mode();

    // Retry init — display may not be connected or needs time after power-on
    loop {
        match display.init() {
            Ok(_) => {
                info!("OLED initialized");
                break;
            }
            Err(_) => {
                warn!("OLED init failed, retrying in 1 s...");
                Timer::after_secs(1).await;
            }
        }
    }

    let style = MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(BinaryColor::On)
        .build();

    let mut counter: u32 = 0;
    let mut buf = heapless::String::<32>::new();
    let mut ticker = Ticker::every(Duration::from_secs(1));

    loop {
        display.clear_buffer();

        // Title
        let _ = Text::new("Polar Pilot", Point::new(16, 10), style)
            .draw(&mut display);

        // Counter
        buf.clear();
        core::write!(buf, "Count: {}", counter).unwrap();
        let _ = Text::new(&buf, Point::new(16, 30), style)
            .draw(&mut display);

        // Spinning dot — cycles through 4 positions
        let positions = [Point::new(100, 45), Point::new(110, 45), Point::new(110, 55), Point::new(100, 55)];
        let pos = positions[(counter as usize) % 4];
        let _ = Circle::new(pos, 6)
            .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
            .draw(&mut display);

        if let Err(_) = display.flush() {
            warn!("OLED flush failed (count={})", counter);
        }

        led.toggle();
        counter = counter.wrapping_add(1);
        ticker.next().await;
    }
}
