//! W5500 Antenna Rotator Controller — Embassy tutorial
//!
//! This firmware demonstrates:
//!   - Embassy async tasks on STM32L4
//!   - SPI communication with a W5500 Ethernet module
//!   - DHCP client + TCP server using embassy-net
//!   - Hamlib rotctld protocol parsing

#![no_std] // No standard library — we're on bare metal
#![no_main] // No fn main() — Embassy provides the entry point

use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_net::StackResources;
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::mode::Async;
use embassy_stm32::rng::Rng;
use embassy_stm32::spi::{self, Spi};
use embassy_stm32::time::Hertz;
use embassy_stm32::{bind_interrupts, peripherals};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_io_async::Write;
use static_cell::StaticCell;
use panic_halt as _;

// ═══════════════════════════════════════════════════════════════════
// CONCEPT: Interrupt binding
// ═══════════════════════════════════════════════════════════════════
// Embassy doesn't use #[interrupt] handlers.  Instead, you *bind*
// interrupt sources to Embassy's internal handlers.  This macro
// generates the vector table entries.
//
// Each line says: "when IRQ X fires, route it to handler Y".
// The HAL drivers then .await on these interrupts internally.
bind_interrupts!(struct Irqs {
    // RNG peripheral — needed to generate a random seed for the TCP stack
    RNG => embassy_stm32::rng::InterruptHandler<peripherals::RNG>;
    // DMA channels used by SPI1 (TX=CH3, RX=CH2)
    DMA1_CHANNEL2 => embassy_stm32::dma::InterruptHandler<peripherals::DMA1_CH2>;
    DMA1_CHANNEL3 => embassy_stm32::dma::InterruptHandler<peripherals::DMA1_CH3>;
    // EXTI3 — W5500 interrupt pin (PA3)
    EXTI3 => embassy_stm32::exti::InterruptHandler<embassy_stm32::interrupt::typelevel::EXTI3>;
});

// ═══════════════════════════════════════════════════════════════════
// CONCEPT: Static state for 'static tasks
// ═══════════════════════════════════════════════════════════════════
// Embassy tasks must own 'static data (they live forever).  You can't
// pass stack-local references to a spawned task.  StaticCell is a
// safe way to create one-time-initialized statics at runtime.
//
// State<N_RX, N_TX> holds the W5500 driver's internal buffers.
// StackResources<N> holds the TCP/IP stack's socket storage.
static W5500_STATE: StaticCell<embassy_net_wiznet::State<2, 2>> = StaticCell::new();
static NET_RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();

// ═══════════════════════════════════════════════════════════════════
// CONCEPT: The #[embassy_executor::main] entry point
// ═══════════════════════════════════════════════════════════════════
// This replaces the usual `fn main()`.  Embassy:
//   1. Sets up the Cortex-M vector table
//   2. Creates the executor (task scheduler)
//   3. Runs this async function as the first task
//
// `spawner` lets you launch additional tasks.
// The return type is `!` (never) — embedded code runs forever.
#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
    // ───────────────────────────────────────────────────────────────
    // CONCEPT: Peripheral initialization
    // ───────────────────────────────────────────────────────────────
    // `embassy_stm32::init(config)` takes ownership of ALL chip
    // peripherals and returns them as a struct.  You then move
    // individual pins/peripherals into the drivers that need them.
    //
    // This is Rust's ownership system applied to hardware: once you
    // give PA5 to the SPI driver, nothing else can use PA5.

    // Configure the clock tree: run the core at 80 MHz from the PLL.
    let mut config = embassy_stm32::Config::default();
    {
        use embassy_stm32::rcc::*;
        // The STM32L432 has a 16 MHz HSI (internal) oscillator.
        // We feed it into the PLL to get 80 MHz for the system clock.
        config.rcc.hsi = true;
        config.rcc.pll = Some(Pll {
            source: PllSource::HSI,   // 16 MHz in
            prediv: PllPreDiv::DIV1,  // 16 MHz to PLL
            mul: PllMul::MUL10,      // 16 × 10 = 160 MHz VCO
            divp: None,
            divq: None,
            divr: Some(PllRDiv::DIV2), // 160 / 2 = 80 MHz
        });
        config.rcc.sys = Sysclk::PLL1_R; // Use PLL R output as SYSCLK
    }
    let p = embassy_stm32::init(config);

    // ───────────────────────────────────────────────────────────────
    // CONCEPT: Async SPI with DMA
    // ───────────────────────────────────────────────────────────────
    // Embassy's SPI driver uses DMA for transfers.  While bytes are
    // moving over the wire, the CPU is free to run other tasks.
    //
    // Spi::new() takes:
    //   - The SPI peripheral (SPI1)
    //   - SCK, MOSI, MISO pins
    //   - Two DMA channels (TX and RX)
    //   - SPI configuration (clock speed, polarity, etc.)
    //
    // On the STM32L432, DMA channels have fixed peripheral mappings:
    // SPI1_RX = DMA1_CH2, SPI1_TX = DMA1_CH3.
    let mut spi_cfg = spi::Config::default();
    spi_cfg.frequency = Hertz(20_000_000); // 20 MHz
    #[allow(unused_mut)] // mut needed only for test-spi feature
    let mut spi = Spi::new(
        p.SPI1, p.PA5, p.PA7, p.PA6, // SCK, MOSI, MISO
        p.DMA1_CH3, p.DMA1_CH2,      // TX DMA, RX DMA
        Irqs,                         // DMA interrupt bindings
        spi_cfg,
    );

    // ───────────────────────────────────────────────────────────────
    // TEST MODE: Read W5500 chip version register in a loop
    // ───────────────────────────────────────────────────────────────
    // Build with: cargo run --release --features test-spi
    //
    // Reads VERSIONR (0x0039) via raw SPI with manual CS toggle.
    // Should return 0x04.  If you get 0x00, check wiring and power.
    #[cfg(feature = "test-spi")]
    {
        use embassy_time::Timer;

        // Reset the W5500: drive RST low for 10ms, then high, wait 100ms
        let mut w5500_reset = Output::new(p.PA2, Level::Low, Speed::VeryHigh);
        Timer::after_millis(10).await;
        w5500_reset.set_high();
        Timer::after_millis(100).await;

        let mut cs = Output::new(p.PA4, Level::High, Speed::VeryHigh);

        info!("=== SPI TEST MODE ===");
        info!("Reading W5500 VERSIONR (0x0039) - expect 0x04");

        let mut iteration: u32 = 0;
        loop {
            // W5500 SPI frame: send 3-byte header + read 1 byte response.
            // We use transfer (simultaneous write+read) with a 4-byte buffer:
            //   TX: [addr_hi, addr_lo, control, 0x00]
            //   RX: [xx,      xx,      xx,      version]
            // Control byte 0x00 = common register block, read mode.
            let mut buf = [0x00u8, 0x39, 0x00, 0x00];
            cs.set_low();
            let result = spi.transfer_in_place(&mut buf).await;
            cs.set_high();

            match result {
                Ok(()) => {
                    let ver = buf[3];
                    if ver == 0x04 {
                        info!("[{}] VERSIONR = 0x{:02x} OK", iteration, ver);
                    } else {
                        warn!("[{}] VERSIONR = 0x{:02x} FAIL (expected 0x04)", iteration, ver);
                    }
                }
                Err(e) => {
                    error!("[{}] SPI error: {:?}", iteration, e);
                }
            }

            iteration += 1;
            Timer::after_millis(1000).await;
        }
    }

    // ─── Normal mode (skipped when test-spi feature is enabled) ──
    #[cfg(not(feature = "test-spi"))]
    {

    // ───────────────────────────────────────────────────────────────
    // CONCEPT: SpiDevice = SPI bus + chip select
    // ───────────────────────────────────────────────────────────────
    // embedded-hal splits SPI into two layers:
    //   SpiBus  — the raw MOSI/MISO/SCK signals
    //   SpiDevice — a bus + CS pin, manages select/deselect
    //
    // ExclusiveDevice wraps our SPI bus + CS into an SpiDevice.
    // "Exclusive" means only one device on the bus (the W5500).
    let cs = Output::new(p.PA4, Level::High, Speed::VeryHigh);
    let spi_dev = ExclusiveDevice::new(spi, cs, Delay).unwrap();

    // ───────────────────────────────────────────────────────────────
    // CONCEPT: ExtiInput — GPIO with async .await for edges
    // ───────────────────────────────────────────────────────────────
    // The W5500 asserts its INT pin when data arrives.  Instead of
    // polling, we use EXTI (External Interrupt) to .await on it.
    // ExtiInput combines a GPIO input + its EXTI channel.
    let w5500_int = embassy_stm32::exti::ExtiInput::new(p.PA3, p.EXTI3, Pull::Up, Irqs);
    let w5500_reset = Output::new(p.PA2, Level::High, Speed::VeryHigh);

    // ───────────────────────────────────────────────────────────────
    // CONCEPT: W5500 driver initialization
    // ───────────────────────────────────────────────────────────────
    // embassy_net_wiznet::new() resets the W5500, configures it over
    // SPI, and returns two things:
    //   - device: implements embassy_net::Driver (send/receive frames)
    //   - runner: a future that must be .await'd to pump the driver
    //
    // This pattern (device + runner) is common in Embassy: the runner
    // is a background loop that you spawn as a separate task.
    let mac_addr = [0x02, 0x00, 0x00, 0x00, 0x00, 0x01];
    let state = W5500_STATE.init(embassy_net_wiznet::State::<2, 2>::new());
    let (device, runner) = embassy_net_wiznet::new::<_, _, embassy_net_wiznet::chip::W5500, _, _, _>(
        mac_addr, state, spi_dev, w5500_int, w5500_reset,
    )
    .await
    .unwrap();

    // Spawn the W5500 driver task (pumps SPI ↔ W5500 in background)
    spawner.spawn(ethernet_task(runner).unwrap());

    // ───────────────────────────────────────────────────────────────
    // CONCEPT: embassy-net Stack
    // ───────────────────────────────────────────────────────────────
    // The Stack wraps a network Device and provides TCP/UDP.
    // It also returns a runner that must be spawned.
    //
    // Static IP configuration — no DHCP.
    // StackResources<3> allows up to 3 simultaneous sockets.
    //
    // The random seed is needed for TCP sequence numbers.
    let net_config = embassy_net::Config::ipv4_static(embassy_net::StaticConfigV4 {
        address: embassy_net::Ipv4Cidr::new(embassy_net::Ipv4Address::new(10, 0, 0, 100), 24),
        gateway: Some(embassy_net::Ipv4Address::new(10, 0, 0, 1)),
        dns_servers: heapless::Vec::new(),
    });

    // Generate a random seed from the hardware RNG
    let mut rng = Rng::new(p.RNG, Irqs);
    let mut seed_bytes = [0u8; 8];
    rng.async_fill_bytes(&mut seed_bytes).await.unwrap();
    let seed = u64::from_le_bytes(seed_bytes);

    let resources = NET_RESOURCES.init(StackResources::new());
    let (stack, runner) = embassy_net::new(device, net_config, resources, seed);

    // Spawn the network stack task (handles ARP, timers, etc.)
    spawner.spawn(net_task(runner).unwrap());

    // Static IP — no DHCP wait needed

    // ───────────────────────────────────────────────────────────────
    // CONCEPT: TCP server loop
    // ───────────────────────────────────────────────────────────────
    // TcpSocket is stack-allocated.  Each call to accept() waits for
    // a client to connect.  When the connection drops, we loop back
    // and accept again.
    //
    // Note: `stack` is Copy — it's a lightweight handle, not the
    // actual stack.  You can pass it freely.
    let mut rx_buf = [0u8; 1024];
    let mut tx_buf = [0u8; 1024];

    loop {
        let mut socket = TcpSocket::new(stack, &mut rx_buf, &mut tx_buf);
        socket.set_timeout(Some(embassy_time::Duration::from_secs(30)));

        if let Err(_) = socket.accept(4533).await {
            continue;
        }

        // Handle this connection
        handle_connection(&mut socket).await;
    }

    } // #[cfg(not(feature = "test-spi"))]
}

// ═══════════════════════════════════════════════════════════════════
// CONCEPT: Spawned tasks
// ═══════════════════════════════════════════════════════════════════
// Each #[embassy_executor::task] is a top-level async function that
// runs concurrently with main and other tasks.  Important rules:
//   - All parameters must be 'static (tasks live forever)
//   - Return type should be `-> !` (run forever)
//   - The executor switches between tasks at .await points

/// Drives the W5500 SPI communication.  Must run continuously.
#[embassy_executor::task]
async fn ethernet_task(
    runner: embassy_net_wiznet::Runner<
        'static,
        embassy_net_wiznet::chip::W5500,
        ExclusiveDevice<Spi<'static, Async, embassy_stm32::spi::mode::Master>, Output<'static>, Delay>,
        embassy_stm32::exti::ExtiInput<'static, Async>,
        Output<'static>,
    >,
) -> ! {
    runner.run().await
}

/// Drives the TCP/IP stack (DHCP, ARP, timers).  Must run continuously.
#[embassy_executor::task]
async fn net_task(
    mut runner: embassy_net::Runner<'static, embassy_net_wiznet::Device<'static>>,
) -> ! {
    runner.run().await
}

// ═══════════════════════════════════════════════════════════════════
// Rotctld protocol handler
// ═══════════════════════════════════════════════════════════════════

/// Rotator state — in-memory stub (no real motors).
struct RotatorState {
    azimuth: f32,
    elevation: f32,
}

static mut ROTATOR: RotatorState = RotatorState {
    azimuth: 0.0,
    elevation: 0.0,
};

/// Handle a single TCP connection, processing rotctld commands.
async fn handle_connection(socket: &mut TcpSocket<'_>) {
    let mut buf = [0u8; 256];
    let mut line_buf = [0u8; 256];
    let mut line_len: usize = 0;

    loop {
        let n = match socket.read(&mut buf).await {
            Ok(0) => return, // EOF — client disconnected
            Ok(n) => n,
            Err(_) => return,
        };

        // Process received bytes, looking for newline-terminated commands
        for &byte in &buf[..n] {
            if byte == b'\n' {
                let line = &line_buf[..line_len];
                if let Err(_) = process_command(line, socket).await {
                    return; // Write error or quit command
                }
                line_len = 0;
            } else if line_len < line_buf.len() {
                line_buf[line_len] = byte;
                line_len += 1;
            }
        }
    }
}

/// Parse and execute a single rotctld command.
/// Returns Err(()) if the connection should be closed.
async fn process_command(line: &[u8], socket: &mut TcpSocket<'_>) -> Result<(), ()> {
    let line = trim(line);

    match line {
        // ── Get position ────────────────────────────────────────
        b"p" | b"\\get_pos" => {
            let (az, el) = unsafe { (ROTATOR.azimuth, ROTATOR.elevation) };
            let mut resp = [0u8; 64];
            let n = format_position(&mut resp, az, el);
            socket.write_all(&resp[..n]).await.map_err(|_| ())?;
        }

        // ── Stop ────────────────────────────────────────────────
        b"S" | b"\\stop" => {
            socket.write_all(b"RPRT 0\n").await.map_err(|_| ())?;
        }

        // ── Get info ────────────────────────────────────────────
        b"_" | b"\\get_info" => {
            socket
                .write_all(b"Model: W5500 Rotator\n")
                .await
                .map_err(|_| ())?;
        }

        // ── Quit ────────────────────────────────────────────────
        b"q" | b"Q" => {
            return Err(()); // Signal to close connection
        }

        // ── Dump state (compatibility) ──────────────────────────
        b"\\dump_state" => {
            socket
                .write_all(
                    b"0\nrot_model=0\nmin_az=0.0\nmax_az=360.0\nmin_el=0.0\nmax_el=90.0\n0\n0\n",
                )
                .await
                .map_err(|_| ())?;
        }

        // ── Set position: "P <az> <el>" ────────────────────────
        _ if line.starts_with(b"P ") || line.starts_with(b"\\set_pos ") => {
            let args = if line.starts_with(b"P ") {
                &line[2..]
            } else {
                &line[9..]
            };
            if let Some((az, el)) = parse_two_floats(args) {
                unsafe {
                    ROTATOR.azimuth = az;
                    ROTATOR.elevation = el;
                }
                socket.write_all(b"RPRT 0\n").await.map_err(|_| ())?;
            } else {
                socket.write_all(b"RPRT -1\n").await.map_err(|_| ())?;
            }
        }

        // ── Unknown command ─────────────────────────────────────
        _ => {
            socket.write_all(b"RPRT -1\n").await.map_err(|_| ())?;
        }
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════
// Helpers — no-std float formatting and parsing
// ═══════════════════════════════════════════════════════════════════

/// Format "az\nel\n" into the buffer.  Returns bytes written.
fn format_position(buf: &mut [u8], az: f32, el: f32) -> usize {
    let mut pos = 0;
    pos += format_f32(&mut buf[pos..], az);
    buf[pos] = b'\n';
    pos += 1;
    pos += format_f32(&mut buf[pos..], el);
    buf[pos] = b'\n';
    pos += 1;
    pos
}

/// Minimal f32 formatter: writes "[-]digits.d" (one decimal place).
fn format_f32(buf: &mut [u8], val: f32) -> usize {
    let mut pos = 0;
    let val = if val < 0.0 {
        buf[pos] = b'-';
        pos += 1;
        -val
    } else {
        val
    };
    let integer = val as u32;
    let frac = ((val - integer as f32) * 10.0) as u8;
    pos += format_u32(&mut buf[pos..], integer);
    buf[pos] = b'.';
    pos += 1;
    buf[pos] = b'0' + frac;
    pos += 1;
    pos
}

/// Format a u32 into decimal ASCII.  Returns bytes written.
fn format_u32(buf: &mut [u8], mut val: u32) -> usize {
    if val == 0 {
        buf[0] = b'0';
        return 1;
    }
    let mut tmp = [0u8; 10];
    let mut len = 0;
    while val > 0 {
        tmp[len] = b'0' + (val % 10) as u8;
        val /= 10;
        len += 1;
    }
    for i in 0..len {
        buf[i] = tmp[len - 1 - i];
    }
    len
}

/// Parse "az el" from a byte slice.
fn parse_two_floats(input: &[u8]) -> Option<(f32, f32)> {
    let input = trim(input);
    // Find the space between the two numbers
    let space = input.iter().position(|&b| b == b' ')?;
    let az = parse_f32(&input[..space])?;
    let el = parse_f32(trim(&input[space + 1..]))?;
    Some((az, el))
}

/// Minimal f32 parser for ASCII decimal like "123.4" or "-5.67".
fn parse_f32(s: &[u8]) -> Option<f32> {
    if s.is_empty() {
        return None;
    }
    let (neg, s) = if s[0] == b'-' { (true, &s[1..]) } else { (false, s) };
    let mut integer: u32 = 0;
    let mut frac: u32 = 0;
    let mut frac_digits: u32 = 0;
    let mut in_frac = false;

    for &b in s {
        if b == b'.' {
            in_frac = true;
        } else if b.is_ascii_digit() {
            if in_frac {
                frac = frac * 10 + (b - b'0') as u32;
                frac_digits += 1;
            } else {
                integer = integer * 10 + (b - b'0') as u32;
            }
        } else {
            return None;
        }
    }

    let mut val = integer as f32;
    if frac_digits > 0 {
        let mut divisor = 1u32;
        for _ in 0..frac_digits {
            divisor *= 10;
        }
        val += frac as f32 / divisor as f32;
    }
    if neg {
        val = -val;
    }
    Some(val)
}

/// Trim leading/trailing whitespace and carriage returns.
fn trim(s: &[u8]) -> &[u8] {
    let mut start = 0;
    while start < s.len() && (s[start] == b' ' || s[start] == b'\r' || s[start] == b'\t') {
        start += 1;
    }
    let mut end = s.len();
    while end > start && (s[end - 1] == b' ' || s[end - 1] == b'\r' || s[end - 1] == b'\t') {
        end -= 1;
    }
    &s[start..end]
}
