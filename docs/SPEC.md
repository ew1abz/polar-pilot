# STM32L432 + W5500 Antenna Rotator Controller

## Overview

Bare-metal Rust firmware ("Polar Pilot") for a two-axis antenna rotator
controller. Runs on an STM32L432 Nucleo-32 board with a W5500 SPI Ethernet
module, SSD1306 OLED display, two stepper motor drivers, a 5-way navigation
button, and two homing endstops. Supports Hamlib rotctld (TCP :4533) and
EasyComm II (USART2 serial), with a polar-chart local display and automatic
homing on power-on.

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

**Peripherals used:** SPI1 (W5500), I2C1 (SSD1306), USART2 (serial/EasyComm),
TIM1 (AZ step), TIM2 (EL step), TIM15 (Embassy time driver), RNG,
DMA1_CH2/CH3 (SPI RX/TX), DMA1_CH6/CH7 (USART2 RX/TX).

### Standalone Operation (No USB)

When powered from an external 3.3 V supply without a USB cable, the ST-LINK
section is unpowered and its nRST output holds the MCU in reset indefinitely.

**Fix**: bridge **CN2 pin 4** (ST-LINK VDD, 3.3 V) to **CN4 pin 14** (main
3.3 V rail) with a jumper wire. This keeps the ST-LINK powered, releases
nRST, and the MCU boots normally without USB.

> CN2 is the 4-pin SWD debug connector; CN4 is the right-side morpho header.

## Software Architecture

### Tasks

See `docs/TASK_ARCHITECTURE.md` for detailed data flow diagrams and
per-task peripheral assignments. Summary:

1. **main** — Inits clocks (80 MHz), peripherals, spawns all tasks, then idles.
2. **ethernet_task** — Pumps W5500 SPI driver (embassy-net-wiznet Runner).
3. **net_task** — Drives TCP/IP stack (DHCP, ARP, timers).
4. **dhcp_watchdog_task** — Waits 120 s for DHCP; falls back to static IP.
5. **motor_task** — Receives commands, slews steppers via TIM1/TIM2 PWM,
   runs homing sequence on power-on, publishes state.
6. **display_task** — Renders polar diagram + status on SSD1306 OLED at 4 Hz.
7. **key_task** — Debounces 5-way nav buttons at 20 ms, sends GoTo/Stop commands.
8. **rotctld_task** — TCP server on :4533, Hamlib rotctld protocol subset.
9. **easycom_task** — USART2 serial, EasyComm II protocol.
10. **led_task** — PB3 heartbeat at 1 Hz.

### Network Configuration

- **DHCP client**: Enabled. Falls back to static IP after 120 s if no offer arrives.
- **Static fallback**: `192.168.1.200/24`, gateway `192.168.1.1`
  (configurable at the top of `src/tasks/net.rs`).
- **MAC address**: Locally-administered, hardcoded (`02:00:00:00:00:01`).
- **Listen port**: 4533 (TCP).

## Rotctld Protocol (Hamlib)

The device implements a subset of the rotctld text protocol. Each command is
a single line terminated by `\n`. Responses are also newline-terminated.

### Supported Commands

| Command                        | Alias              | Description              | Response                     |
|--------------------------------|--------------------|--------------------------|------------------------------|
| `p`                            | `\get_pos`         | Get position             | `<az>\n<el>\n`               |
| `P <az> <el>`                  | `\set_pos <az> <el>` | Set position           | `RPRT 0\n`                   |
| `S`                            | `\stop`            | Stop rotation            | `RPRT 0\n`                   |
| `l`                            | `\get_limits`      | Get soft limits          | `az_min\naz_max\nel_min\nel_max\nRPRT 0\n` |
| `L <az_min> <az_max> <el_min> <el_max>` | `\set_limits …` | Set soft limits | `RPRT 0\n`          |
| `q`                            | `\quit`            | Close connection         | (connection closed)          |
| `_`                            | `\get_info`        | Get info                 | `Model: Polar Pilot\n`       |
| `\dump_state`                  | —                  | Hamlib compatibility     | State block with current limits |

When `Phase::Fault`, `p`, `P`, and `S` all respond with:
```text
FAULT: <message>
RPRT -9
```
`RPRT -9` is Hamlib's "command rejected" code. Unrecognized commands return `RPRT -1\n`.

### `\dump_state` Response

The `min_az/max_az/min_el/max_el` fields reflect the current soft limits:

```text
0
rot_model=0
min_az=<az_min>
max_az=<az_max>
min_el=<el_min>
max_el=<el_max>
0
0
```

## EasyComm II Protocol

| Command                           | Description                        | Response                          |
|-----------------------------------|------------------------------------|-----------------------------------|
| `AZ`                              | Query position                     | `AZ<az> EL<el>\n`                 |
| `AZ<az> EL<el>`                   | Set position                       | (none)                            |
| `SA` / `SE` / `SA SE`             | Stop                               | (none)                            |
| `LM`                              | Get soft limits                    | `LM az_min az_max el_min el_max\n`|
| `LM <az_min> <az_max> <el_min> <el_max>` | Set soft limits           | (none)                            |
| `VE`                              | Version query                      | `Polar Pilot v0.1\n`              |

## Development

Build instructions, flash commands, hardware test binaries, known issues, and
the protocol test suite are documented in [docs/DEVELOPMENT.md](DEVELOPMENT.md).
