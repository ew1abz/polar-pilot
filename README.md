# Polar Pilot

<p align="center">
  <img src="docs/polar-pilot.png" alt="Polar Pilot logo" width="160"/>
</p>

<p align="center">
  <em>Bare-metal Rust firmware for a satellite antenna rotator controller — Hamlib + EasyComm II over Ethernet, live polar display, no OS.</em>
</p>

<p align="center">
  <a href="https://github.com/ew1abz/polar-pilot/actions"><img src="https://github.com/ew1abz/polar-pilot/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <img src="https://img.shields.io/badge/target-thumbv7em--none--eabihf-blue" alt="target">
  <img src="https://img.shields.io/badge/embassy-async-orange" alt="embassy">
  <img src="https://img.shields.io/badge/license-GPL--3.0-green" alt="license">
</p>

<p align="center">
  <a href="https://github.com/ew1abz/polar-pilot">
    <img src="https://gh-card.dev/repos/ew1abz/polar-pilot.svg" alt="Repo card">
  </a>
</p>

---

## Overview

Polar Pilot was originally designed as a controller for the
[SatNOGS rotator](https://wiki.satnogs.org/SatNOGS_Rotator_v3) — the
open-source satellite antenna rotator from the Libre Space Foundation. It has
since grown into a general-purpose two-axis rotator controller.

Polar Pilot turns an STM32L432KC Nucleo-32 board into a full-featured antenna
rotator controller. It speaks both **Hamlib rotctld** (TCP port 4533) and
**EasyComm II** (USART2 serial) so it integrates directly with
[gpredict](http://gpredict.oz9aec.net/), [Orbitron](https://www.stoff.pl/),
or any rotator-aware logging software out of the box.

Networking is handled by a W5500 hardwired TCP/IP module over SPI. A 128×64
OLED shows a live polar diagram of current and target azimuth/elevation. A
5-way navigation button lets you jog the rotator manually without a computer.

The entire firmware is `no_std` / `no_main` — no RTOS, no heap, no Linux. Nine
concurrent async tasks run on the [Embassy](https://embassy.dev/) executor.

## Features

- **Dual interface** — Ethernet (TCP :4533) and serial (USART2) operate simultaneously
- **Dual serial protocol** — EasyComm II and GS-232/GS-232B commands on the same USART2 port
- **Dual TCP connection** — up to two concurrent rotctld clients on port 4533
- **W5500 Ethernet** — ICMP (ping), DHCP, hardwired TCP/IP stack
- **[SSD1306 polar display](docs/OLED_POLAR_DISPLAY.md)** — real-time azimuth/elevation polar chart at 4 Hz
- **Stepper motor control** — hardware PWM via TIM1/TIM2, STEP/DIR/EN interface
- **[Auto-homing](docs/HOMING.md)** — endstop-based homing on power-on
- **Soft limits** — configurable AZ/EL travel limits via rotctld (`L`) and EasyComm (`LM`), persisted across commands
- **[5-way joystick](docs/MANUAL_CONTROL.md)** — manual GoTo and Stop commands without a PC
- **Heartbeat LED** — 1 Hz blink on PB3 to confirm the firmware is alive
- **Embassy async** — nine cooperative tasks, no RTOS, no heap

## Hardware

The firmware is flexible — W5500, SSD1306, stepper drivers, and the 5-way
button can each be omitted. The system boots and operates with whatever subset
is physically connected; missing peripherals are detected at runtime and their
tasks degrade gracefully without affecting the rest.

| Component    | Part           | Required | Notes                            |
|--------------|----------------|----------|----------------------------------|
| MCU          | STM32L432KC    | Yes      | Cortex-M4F, 80 MHz, 256 KB flash |
| Board        | Nucleo-L432KC  | Yes      | Nucleo-32, ST-Link v2 onboard    |
| Ethernet     | W5500 module   | No       | Rotctld TCP server disabled if absent |
| Display      | SSD1306        | No       | 128×64 OLED, I2C; retries until connected |
| AZ stepper   | A4988 or equiv | Yes      | STEP/DIR/EN                      |
| EL stepper   | A4988 or equiv | Yes      | STEP/DIR/EN                      |
| Navigation   | 5-way button   | No       | Pull-ups hold idle if unplugged  |
| Endstops     | Microswitch ×2 | Yes      | Homing faults without them       |

### Required Nucleo board modifications

Four 0 Ω solder bridges must be removed before assembling:

| Bridge | Why                                                                     |
|--------|-------------------------------------------------------------------------|
| SB1    | Frees PC14 from the 32 kHz oscillator -> AZ DIR output                  |
| SB2    | Frees PC15 from the 32 kHz oscillator -> Nav CENTER input               |
| SB16   | Isolates PA5 from PB6 so SPI1 (W5500) and I2C1 (SSD1306) don't short    |
| SB18   | Isolates PA6 from PB7 for the same reason                               |

See [docs/SPEC.md](docs/SPEC.md) for the full pin table and a standalone-power
jumper note (needed when running without USB).

### Schematic

The KiCad schematic is in [kikad/polar-pilot.kicad_sch](kikad/polar-pilot.kicad_sch).
A rendered PDF export is at [kikad/polar-pilot.pdf](kikad/polar-pilot.pdf).

## Getting Started

### Prerequisites

```bash
rustup target add thumbv7em-none-eabihf
cargo install probe-rs-tools
```

### Build and flash

```bash
cargo build --release        # build only
cargo run --release          # build + flash + RTT console
```

### Hardware test binaries

```bash
cargo run --release --bin w5500_test    # W5500 chip version register
cargo run --release --bin oled_test     # OLED display
cargo run --release --bin motor_test    # stepper motion
cargo run --release --bin endstop_test  # endstop inputs
cargo run --release --bin button_test   # 5-way nav buttons
```

### Connecting gpredict

1. Flash the firmware and connect the W5500 module to your network.
2. Watch the LCD for the DHCP-assigned IP address.
3. In gpredict → Edit → Preferences → Interfaces → Rotators, add a new
   rotator: **Host** = `<ip>`, **Port** = `4533`.
4. Start tracking — gpredict will drive the rotator via rotctld protocol.

## Architecture

Nine Embassy async tasks communicate through two shared primitives:

```text
[rotctld TCP :4533] ──┐
[easycom USART2]  ────┤  RotatorCmd   ┌────────────┐
[5-way nav keys]  ────┴──────────────►│ motor_task ├──► Watch<RotatorState>
                                      └────────────┘
                                            │
                        ┌───────────────────┼───────────────┐
                        ▼                   ▼               ▼
                  display_task        ethernet_task      led_task
```

- **`Watch<RotatorState>`** — single-producer (motor_task), multi-consumer
  broadcast of current and target position
- **`Channel<RotatorCmd>`** — multi-producer (rotctld, easycom, keys),
  single-consumer (motor_task) command funnel

Full data-flow diagrams and per-task peripheral assignments are in
[docs/TASK_ARCHITECTURE.md](docs/TASK_ARCHITECTURE.md).

## Roadmap

- [ ] 540° / 1.5-rotation overtravel mode (eliminates the north-crossing dead zone)
- [ ] Screen modes: polar chart / big AZ+EL digits / info (IP, version, GitHub)
- [ ] Rust TUI companion application
- [ ] Rust/WASM web companion app

## License

Copyright (C) 2026 ew1abz

This program is free software: you can redistribute it and/or modify it under
the terms of the **GNU General Public License v3.0** as published by the Free
Software Foundation.

See [LICENSE](LICENSE) for the full text.
