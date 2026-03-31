use core::fmt::Write as FmtWrite;

use defmt::*;
use embassy_net::Stack;
use embassy_stm32::i2c::I2c;
use embassy_time::{Duration, Ticker, Timer};
use embedded_graphics::image::{Image, ImageRaw};
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::mono_font::MonoTextStyleBuilder;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Circle, Line, PrimitiveStyle};
use embedded_graphics::text::Text;
use ssd1306::prelude::*;
use ssd1306::rotation::DisplayRotation;
use ssd1306::size::DisplaySize128x64;
use ssd1306::I2CDisplayInterface;
use ssd1306::Ssd1306;

use crate::types::{Phase, STATE};

#[embassy_executor::task]
pub async fn display_task(
    i2c: I2c<'static, embassy_stm32::mode::Blocking, embassy_stm32::i2c::Master>,
    stack: Stack<'static>,
) -> ! {
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
    display.set_brightness(Brightness::BRIGHTEST).ok();

    // ── Splash screen ──────────────────────────────────────────
    {
        display.clear_buffer();
        let raw: ImageRaw<BinaryColor> = ImageRaw::new(include_bytes!("../rust.raw"), 64);
        let _ = Image::new(&raw, Point::new(0, 0)).draw(&mut display);
        let splash_style = MonoTextStyleBuilder::new()
            .font(&FONT_6X10)
            .text_color(BinaryColor::On)
            .build();
        let _ = Text::new("Polar", Point::new(74, 24), splash_style).draw(&mut display);
        let _ = Text::new("Pilot", Point::new(74, 38), splash_style).draw(&mut display);
        let _ = Text::new(
            concat!("v", env!("CARGO_PKG_VERSION")),
            Point::new(70, 56),
            splash_style,
        )
        .draw(&mut display);
        display.flush().ok();
        Timer::after_secs(2).await;
    }

    let thin_stroke = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
    let text_style = MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(BinaryColor::On)
        .build();

    const CX: i32 = 39;
    const CY: i32 = 32;
    const R_OUTER: i32 = 30;
    const R_INNER: i32 = 15;
    const PI: f32 = core::f32::consts::PI;
    const TX: i32 = 80;

    // Airplane pixel offsets: cross arms + 3×3 filled centre
    const PLANE: [(i32, i32); 17] = [
                          (0, -3), (0, -2),
        (-3, 0), (-2, 0),
        (-1, -1), (0, -1), (1, -1),
        (-1,  0), (0,  0), (1,  0),
        (-1,  1), (0,  1), (1,  1),
                   (2, 0), (3, 0),
                          (0,  2), (0,  3),
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

        // ── Fault screen ────────────────────────────────────────
        if let Phase::Fault(msg) = state.phase {
            let _ = Text::new("FAULT",        Point::new(0,  12), text_style).draw(&mut display);
            let _ = Text::new(msg,            Point::new(0,  32), text_style).draw(&mut display);
            let _ = Text::new("Power cycle", Point::new(0,  50), text_style).draw(&mut display);
            let _ = Text::new("to reset",    Point::new(0,  60), text_style).draw(&mut display);
            display.flush().ok();
            continue;
        }

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

        // Cardinal labels — N/S on the left margin, E/W flanking the circle
        let _ = Text::new("N", Point::new(0, 7), text_style).draw(&mut display);
        let _ = Text::new("S", Point::new(0, 63), text_style).draw(&mut display);
        let _ = Text::new("E", Point::new(CX + R_OUTER + 3, CY + 3), text_style).draw(&mut display);
        let _ = Text::new("W", Point::new(CX - R_OUTER - 9, CY + 3), text_style).draw(&mut display);

        // Airplane position.
        // EL > 90° produces a negative r, which naturally places the dot on the
        // opposite azimuth at the mirrored radius — no explicit fold-over needed.
        let angle_rad = state.current_az * PI / 180.0;
        let r = (R_OUTER as f32) * (1.0 - state.current_el / 90.0);
        let px = CX as f32 + r * libm::sinf(angle_rad);
        let py = CY as f32 - r * libm::cosf(angle_rad);
        let px_i = px as i32;
        let py_i = py as i32;

        for &(dx, dy) in &PLANE {
            let _ = Pixel(Point::new(px_i + dx, py_i + dy), BinaryColor::On)
                .draw(&mut display);
        }

        // AZ / EL readout
        buf.clear();
        let _ = core::write!(buf, "AZ:{:3}", state.current_az as i32);
        let _ = Text::new(&buf, Point::new(TX, 10), text_style).draw(&mut display);

        buf.clear();
        let _ = core::write!(buf, "EL:{:3}", state.current_el as i32);
        let _ = Text::new(&buf, Point::new(TX, 22), text_style).draw(&mut display);

        // Status line (above IP)
        let ip_cfg = stack.config_v4();
        let status = match state.phase {
            Phase::Homing => "Homing",
            Phase::Running if state.moving => "Moving",
            Phase::Running if ip_cfg.is_some() => "Idle",
            Phase::Running => "No IP",
            Phase::Fault(_) => core::unreachable!(), // handled above
        };
        let _ = Text::new(status, Point::new(TX, 40), text_style).draw(&mut display);

        // IP address — split across two rows (8 chars each fits the right panel)
        // Row 1: "A.B."  Row 2: "C.D"
        buf.clear();
        if let Some(cfg) = ip_cfg {
            let o = cfg.address.address().octets();
            let _ = core::write!(buf, "{}.{}.", o[0], o[1]);
        } else {
            let _ = buf.push_str("---.---.");
        }
        let _ = Text::new(&buf, Point::new(TX, 52), text_style).draw(&mut display);

        buf.clear();
        if let Some(cfg) = stack.config_v4() {
            let o = cfg.address.address().octets();
            let _ = core::write!(buf, "{}.{}", o[2], o[3]);
        } else {
            let _ = buf.push_str("---.---");
        }
        let _ = Text::new(&buf, Point::new(TX, 62), text_style).draw(&mut display);

        if display.flush().is_err() {
            warn!("OLED flush failed");
        }
    }
}
