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

## Constraints & Decisions

- **No `std`** — bare-metal `#![no_std]`, `#![no_main]`.
- **No heap** — all buffers are stack-allocated or static.
- **Single TCP connection** — the W5500 has 8 sockets but one is sufficient
  for rotctld; keeps the code simple.
- **Stub rotator state** — azimuth/elevation are stored in-memory only.
  Actual motor control is out of scope for this example.
- **No TLS** — rotctld is plaintext.
