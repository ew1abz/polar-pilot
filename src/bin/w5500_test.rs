//! W5500 SPI version register test — resets the W5500 and reads VERSIONR.
//!
//! Pins:
//!   SPI1: PA5/SCK, PA6/MISO, PA7/MOSI, PA4/CS (DMA1_CH2 RX, DMA1_CH3 TX)
//!   W5500: PA1/RST, PA3/INT
//!
//! Expected output: "W5500 detected (VERSIONR = 0x04)"
//! LED blinks on success, stays solid on failure.
//!
//! Run with:  cargo run --release --bin w5500_test

#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::spi::{self, Spi};
use embassy_stm32::time::Hertz;
use embassy_time::{Duration, Ticker, Timer};
use {defmt_rtt as _, panic_probe as _};

embassy_stm32::bind_interrupts!(struct Irqs {
    DMA1_CHANNEL2 => embassy_stm32::dma::InterruptHandler<embassy_stm32::peripherals::DMA1_CH2>;
    DMA1_CHANNEL3 => embassy_stm32::dma::InterruptHandler<embassy_stm32::peripherals::DMA1_CH3>;
});

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

    info!("W5500 SPI test starting...");

    // ── Hardware reset ────────────────────────────────────────────
    let mut w5500_rst = Output::new(p.PA1, Level::Low, Speed::Low);
    Timer::after_millis(1).await;   // hold RST low ≥500 µs per datasheet
    w5500_rst.set_high();
    Timer::after_millis(2).await;   // PLL lock after reset release

    // ── SPI1 init ─────────────────────────────────────────────────
    let mut spi_cfg = spi::Config::default();
    spi_cfg.frequency = Hertz(1_000_000);
    let mut spi = Spi::new(
        p.SPI1, p.PA5, p.PA7, p.PA6,
        p.DMA1_CH3, p.DMA1_CH2,
        Irqs, spi_cfg,
    );
    let mut cs = Output::new(p.PA4, Level::High, Speed::VeryHigh);

    // ── Read VERSIONR (0x0039) ────────────────────────────────────
    // Common register block (BSB=0, read, VDM)
    let cmd = [0x00u8, 0x39, 0x00]; // addr_hi, addr_lo, control
    let mut ver = [0u8; 1];
    let mut attempts = 0u32;

    while ver[0] != 0x04 {
        cs.set_low();
        spi.write(&cmd).await.unwrap();
        spi.read(&mut ver).await.unwrap();
        cs.set_high();

        attempts += 1;
        if attempts % 100 == 0 {
            warn!("Still waiting for W5500... (attempt {}, got {:#x})", attempts, ver[0]);
        }
        Timer::after_millis(2).await;
    }

    info!("W5500 detected (VERSIONR = {:#x}) after {} attempts", ver[0], attempts);

    // ── Success: blink LED ────────────────────────────────────────
    let mut ticker = Ticker::every(Duration::from_millis(200));
    loop {
        ticker.next().await;
        led.toggle();
    }
}
