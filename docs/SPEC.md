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
| Board           | Nucleo-L432KC    | Nucleo-32 form factor, ST-Link v2  |
| Ethernet        | W5500            | Hardwired TCP/IP, SPI interface    |
| Display         | SSD1306          | 128×64 OLED, I2C                   |
| AZ stepper      | A4988 or similar | STEP/DIR/EN interface              |
| EL stepper      | A4988 or similar | STEP/DIR/EN interface              |
| Navigation      | 5-way button     | UP/DOWN/LEFT/RIGHT/CENTER          |
| Endstops        | Microswitch ×2   | Active-low, internal pull-up       |

### Pin Assignments

Four 0 Ω resistors on the Nucleo board must be removed:

| Resistor | Default | Connects    | Why remove                          |
|----------|---------|-------------|-------------------------------------|
| SB1      | ON      | PC14 to OSC | Free PC14 for AZ DIR (no 32k LSE)   |
| SB2      | ON      | PC15 to OSC | Free PC15 for Nav CENTER (no LSE)   |
| SB16     | ON      | PA5 ↔ PB6   | Isolate SPI1_SCK from I2C1_SCL      |
| SB18     | ON      | PA6 ↔ PB7   | Isolate SPI1_MISO from I2C1_SDA     |

SB16/SB18 bridge the SPI1 and I2C1 pins. Without removing them, SPI1 (W5500)
and I2C1 (SSD1306) cannot operate simultaneously. See UM1956 §7.10.

| Pin  | Label | Function   | Peripheral   | Notes                         |
|------|-------|------------|--------------|-------------------------------|
| PA0  | A0    | EL STEP    | TIM2_CH1     | HW pulse generation           |
| PA1  | A1    | W5500 RST  | GPIO output  |                               |
| PA3  | A2    | W5500 INT  | EXTI3        |                               |
| PA4  | A3    | SPI1 CS    | GPIO output  | W5500 chip select             |
| PA5  | A4    | SPI1 SCK   | SPI1         | W5500                         |
| PA6  | A5    | SPI1 MISO  | SPI1         | W5500                         |
| PA7  | A6    | SPI1 MOSI  | SPI1         | W5500                         |
| PA2  | A7    | USART2 TX  | USART2 (AF7) | ST-LINK virtual COM           |
| PA10 | D0    | AZ Home    | GPIO input   | Pull-up, active-low endstop   |
| PA9  | D1    | EL DIR     | GPIO output  |                               |
| PA12 | D2    | Nav RIGHT  | GPIO input   | Pull-up                       |
| PB0  | D3    | Nav DOWN   | GPIO input   | Pull-up                       |
| PB7  | D4    | I2C1_SDA   | I2C1         | SSD1306 display               |
| PB6  | D5    | I2C1_SCL   | I2C1         | SSD1306 display               |
| PB1  | D6    | Nav UP     | GPIO input   | Pull-up                       |
| PC14 | D7    | AZ DIR     | GPIO output  |                               |
| PC15 | D8    | Nav CENTER | GPIO input   | Pull-up                       |
| PA8  | D9    | AZ STEP    | TIM1_CH1     | HW pulse generation           |
| PA11 | D10   | Nav LEFT   | GPIO input   | Pull-up                       |
| PB5  | D11   | EL Home    | GPIO input   | Pull-up, active-low endstop   |
| PB4  | D12   | Motor EN   | GPIO output  | Shared stepper EN, active-low |
| PB3  | D13   | Heartbeat  | GPIO output  | Onboard LED (LD3)             |
| PA15 | —     | USART2 RX  | USART2 (AF3) | ST-LINK VCP, not on headers   |

**Peripherals used:** SPI1 (W5500), I2C1 (SSD1306), USART2 (serial/Easycom),
TIM2 (EL step), TIM1 (AZ step), RNG, DMA1_CH2/CH3 (SPI RX/TX).

## Software Architecture

### Crate Dependencies

| Crate                | Purpose                                |
|----------------------|----------------------------------------|
| `embassy-executor`   | Async task executor                    |
| `embassy-stm32`      | HAL for STM32L4 (SPI, I2C, GPIO, …)   |
| `embassy-time`       | Timekeeping (TIM15 driver, Ticker)     |
| `embassy-sync`       | Watch + Channel for inter-task comms   |
| `embassy-net`        | TCP/IP stack (TCP, DHCPv4)             |
| `embassy-net-wiznet` | W5500 driver for embassy-net           |
| `embedded-hal-bus`   | SPI ExclusiveDevice wrapper            |
| `embedded-io-async`  | Async I/O traits (Write::write_all)    |
| `ssd1306`            | SSD1306 OLED driver                    |
| `embedded-graphics`  | 2D drawing primitives for OLED         |
| `heapless`           | Fixed-capacity String/Vec (no alloc)   |
| `libm`               | Software float math (sinf, cosf)       |
| `defmt` + `defmt-rtt`| Logging via RTT                        |
| `panic-probe`        | Panic handler for probe-based debug    |
| `static_cell`        | One-time `'static` init for task data  |
| `cortex-m` / `cortex-m-rt` | Runtime + vector table           |

### Tasks

See `docs/TASK_ARCHITECTURE.md` for detailed data flow diagrams and
per-task peripheral assignments. Summary:

1. **main** — Inits clocks (80 MHz), peripherals, spawns all tasks, then idles.
2. **ethernet_task** — Pumps W5500 SPI driver (embassy-net-wiznet Runner).
3. **net_task** — Drives TCP/IP stack (DHCP, ARP, timers).
4. **motor_task** — Receives commands, slews steppers via TIM1/TIM2
   PWM, checks endstops, publishes state.
5. **display_task** — Renders polar diagram + status on SSD1306 OLED at 4 Hz.
6. **key_task** — Debounces 5-way nav buttons at 20 ms, sends GoTo/Stop commands.
7. **rotctld_task** — TCP server on :4533, Hamlib rotctld protocol subset.
8. **easycom_task** — USART2 serial, EasyComm II protocol.
9. **led_task** — PB3 heartbeat at 1 Hz.

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
| `_`              | Get info                 | `Model: Polar Pilot\n`       |
| `\dump_state`    | Dump state (compatibility) | Minimal state block        |

Unrecognized commands return `RPRT -1\n`.

### `\dump_state` Response

```text
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

```bash
rustup target add thumbv7em-none-eabihf
cargo install probe-rs-tools
```

### Build

```bash
cargo build --release
```

### Flash + RTT Log

```bash
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

```text
.
├── .cargo/config.toml              # target, runner, DEFMT_LOG
├── build.rs                        # memory.x linker script setup
├── memory.x                        # Flash/RAM layout for STM32L432
├── Cargo.toml
├── src/
│   ├── main.rs                     # all 9 async tasks (single-file firmware)
│   └── bin/                        # standalone hardware test binaries
│       ├── w5500_test.rs
│       ├── button_test.rs
│       ├── motor_test.rs
│       └── oled_test.rs
├── docs/
│   ├── SPEC.md                     # this file
│   ├── TASK_ARCHITECTURE.md        # task data flow and peripheral map
│   └── OLED_POLAR_DISPLAY.md       # OLED rendering details
└── embassy-net-wiznet-patch/       # local W5500 driver patches
```

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
SPI1 must use DMA1_CH2 (RX) and DMA1_CH3 (TX); USART2 must use DMA1_CH7 (TX)
and DMA1_CH6 (RX). Using wrong channels causes silent failures.

## Constraints & Decisions

- **No `std`** — bare-metal `#![no_std]`, `#![no_main]`.
- **No heap** — all buffers are stack-allocated or static.
- **Single TCP connection** — the W5500 has 8 sockets but one is sufficient
  for rotctld; keeps the code simple.
- **No TLS** — rotctld is plaintext.
