# STM32L432 + W5500 Antenna Rotator Controller

## Overview

Embedded Rust firmware for an antenna rotator controller using the Hamlib
network protocol (rotctld). Runs on an STM32L432 Nucleo-32 board with a
W5500 SPI Ethernet module. The device obtains an IP address via DHCP and
listens for rotctld-compatible TCP connections on port 4533.

## Hardware

| Component       | Part             | Notes                              |
|-----------------|------------------|------------------------------------|
| MCU             | STM32L432KC      | Cortex-M4F, 80 MHz, 256 KB flash   |
| Ethernet        | W5500            | Hardwired TCP/IP, SPI interface    |
| Board           | Nucleo-L432KC    | Nucleo-32 form factor, ST-Link v2  |

### Pin Assignments (SPI1)

All signals are on the CN3 (right) header, pins A2–A7 — six consecutive pins.

| Function | STM32 Pin | Nucleo-32 Label | W5500 Pin |
|----------|-----------|-----------------|-----------|
| RST      | PA2       | A7              | RSTn      |
| MOSI     | PA7       | A6              | MOSI      |
| MISO     | PA6       | A5              | MISO      |
| SCK      | PA5       | A4              | SCLK      |
| CS       | PA4       | A3              | SCSn      |
| INT      | PA3       | A2              | INTn      |

## Software Architecture

### Crate Dependencies

| Crate                | Purpose                                |
|----------------------|----------------------------------------|
| `embassy-stm32`      | HAL + async executor for STM32L4       |
| `embassy-executor`   | Async task executor                    |
| `embassy-net`        | Networking stack (TCP, DHCP)           |
| `embassy-net-wiznet` | W5500 driver for embassy-net           |
| `defmt` + `defmt-rtt`| Logging via RTT                        |
| `panic-probe`        | Panic handler for probe-based debug    |

### Tasks

1. **Main task** — Initializes peripherals (SPI, GPIO), creates the W5500
   driver, starts the network stack with DHCP, then spawns the other tasks.

2. **Network task** (`embassy_net::Stack::run`) — Drives the network stack,
   handles DHCP lease acquisition and renewal.

3. **TCP server task** — Listens on port 4533. Accepts one connection at a
   time. Parses and responds to rotctld commands.

### Network Configuration

- **DHCP client**: Enabled. No static fallback.
- **MAC address**: Locally-administered, hardcoded (e.g. `02:00:00:00:00:01`).
- **Listen port**: 4533 (TCP).

## Rotctld Protocol (Hamlib)

The device implements a subset of the rotctld text protocol. Each command is
a single line terminated by `\n`. Responses are also newline-terminated.

### Supported Commands

| Command          | Description              | Response Format              |
|------------------|--------------------------|------------------------------|
| `p`              | Get position             | `<azimuth>\n<elevation>\n`   |
| `P <az> <el>`    | Set position             | `RPRT 0\n` on success        |
| `S`              | Stop rotation            | `RPRT 0\n`                   |
| `q`              | Quit (close connection)  | (connection closed)          |
| `_`              | Get info                 | `Model: W5500 Rotator\n`     |
| `\dump_state`    | Dump state (compatibility) | Minimal state block        |

Unrecognized commands return `RPRT -1\n`.

### `\dump_state` Response

```
0
rot_model=0
min_az=0.0
max_az=360.0
min_el=0.0
max_el=90.0
0
0
```

## Build & Flash

### Prerequisites

```
rustup target add thumbv7em-none-eabihf
cargo install probe-rs-tools
```

### Build

```
cargo build --release
```

### Flash + RTT Log

```
cargo run --release
```

(Configured via `.cargo/config.toml` to use `probe-rs run`.)

### Binary Size

Release build (`opt-level = "s"`, LTO):

| Configuration                        | .text (bytes) | Delta               |
|--------------------------------------|---------------|---------------------|
| Minimal (TCP + static IP, no defmt)  | 42,312        | baseline            |
| Minimal (TCP + static IP, defmt)     | 66,384        | +24,072 (+56.9%)    |
| + DHCP                               | 75,672        | +33,360 (+78.8%)    |
| + DHCP + ICMP echo reply             | 78,112        | +35,800 (+84.6%)    |

STM32L432KC has 256 KB flash — plenty of headroom. The `minimal` branch
has the static-IP-only builds (with and without defmt) for reference.

## Project Structure

```
.
├── .cargo/
│   └── config.toml        # target, runner, linker settings
├── build.rs               # memory.x linker script setup
├── memory.x               # Flash/RAM layout for STM32L432
├── Cargo.toml
├── SPEC.md
└── src/
    └── main.rs            # all tasks: init, net, tcp server, rotctld parser
```

Single-file `main.rs` for simplicity — this is a test/example project.

## Known Issues & Workarounds

### SPI CLK Hi-Z Between DMA Operations (embassy-stm32 0.6.0)

The `embassy-stm32` v0.6.0 SPI driver disables the SPI peripheral (`SPE=0`)
between DMA operations within a single `SpiDevice::transaction()`. On STM32L4,
this causes the SCK pin to go hi-Z, which corrupts communication with the W5500.

**Symptoms**: PHYCFGR reads incorrect values, `is_link_up()` returns false,
DHCP never completes, integer underflow panic in `read_frame()`.

**Hardware workaround**: Add a **pull-down resistor on SCK (PA5)** to hold it
low during hi-Z gaps. This keeps the clock at the correct idle level (CPOL=0)
and prevents the W5500 from seeing spurious clock edges.

**Software patches** (in `embassy-net-wiznet-patch/`):

- Combined the 3-byte SPI header into a single `Operation::Write` (reduces
  operation boundaries from 2 to 1).
- Added bounds check in `read_frame()` to guard against integer underflow
  when a corrupted frame header reports size < 2.
- Removed redundant software reset (`MR=0x80`) which clears PHYCFGR bit 7
  and breaks link detection after the hardware reset already ran.

See `embassy-spi-bug-report` branch for the full upstream bug reports.

### STM32L432 Fixed DMA Channel Mapping

The STM32L432 has no DMAMUX — DMA channels are hardwired per peripheral.
SPI1 must use DMA1_CH2 (RX) and DMA1_CH3 (TX). Using wrong channels causes
silent failures.

## Constraints & Decisions

- **No `std`** — bare-metal `#![no_std]`, `#![no_main]`.
- **No heap** — all buffers are stack-allocated or static.
- **Single TCP connection** — the W5500 has 8 sockets but one is sufficient
  for rotctld; keeps the code simple.
- **Stub rotator state** — azimuth/elevation are stored in-memory only.
  Actual motor control is out of scope for this example.
- **No TLS** — rotctld is plaintext.
