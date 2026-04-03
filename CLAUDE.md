# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when
working with code in this repository.

## What This Is

Bare-metal Rust firmware ("Polar Pilot") for an antenna rotator
controller on STM32L432KC Nucleo-32 + W5500 SPI Ethernet. Speaks
both Hamlib rotctld (TCP :4533) and EasyComm II (USART2 serial).
Single-binary `no_std` Embassy async project with hardware stepper
motor control, SSD1306 OLED polar display, and 5-way button
navigation.

## Build Commands

```bash
cargo build --release                  # build firmware
cargo run --release                    # flash via probe-rs + RTT
cargo run --release --bin w5500_test   # hardware test binary
cargo run --release --bin button_test  # also: motor_test, oled_test
```

Requires: `rustup target add thumbv7em-none-eabihf` and
`cargo install probe-rs-tools`.
There are no unit tests — this is a hardware-only embedded project.
Test binaries in `src/bin/` exercise individual peripherals.

## Architecture

**Nine concurrent async tasks** on the Embassy executor,
communicating via two shared primitives:

- `Watch<RotatorState>` — single-producer (motor_task),
  multi-consumer broadcast of current/target position
- `Channel<RotatorCmd>` — multi-producer (rotctld, easycom, keys),
  single-consumer (motor_task) command funnel

```text
[rotctld TCP :4533]──┐
[easycom USART2]─────┤  RotatorCmd   ┌────────────┐
[5-way nav keys]─────┴──────────────►│ motor_task ├──►Watch
                                     └────────────┘
```

**Task inventory:**

1. **main** — inits clocks (80 MHz HSI+PLL), peripherals,
   spawns all tasks, then idles
2. **motor_task** — receives commands, slews steppers via
   TIM1/TIM2 PWM, checks endstops, publishes state
3. **display_task** — renders polar diagram + status on SSD1306
   OLED at 4 Hz (I2C1)
4. **key_task** — debounces 5-way nav buttons at 20 ms, sends
   GoTo/Stop commands
5. **rotctld_task** — TCP server on :4533, parses Hamlib rotctld
   protocol subset
6. **easycom_task** — USART2 serial, parses EasyComm II protocol
   (AZ/EL/SA/SE/VE)
7. **ethernet_task** — pumps W5500 SPI driver
   (embassy-net-wiznet Runner)
8. **net_task** — drives TCP/IP stack (DHCP, ARP, timers)
9. **led_task** — PB3 heartbeat at 1 Hz

**Source layout:**

```text
src/
  main.rs           — clocks, peripheral init, task spawning
  types.rs          — RotatorState, RotatorCmd, Phase, STATE/CMD statics
  util.rs           — parse_f32, parse_f32_bytes, line_contains, extract_value_after
  tasks/
    motor.rs        — motor_task
    display.rs      — display_task
    keys.rs         — key_task
    rotctld.rs      — rotctld_task
    easycom.rs      — easycom_task
    net.rs          — ethernet_task, net_task, led_task
```

See `docs/TASK_ARCHITECTURE.md` for detailed data flow diagrams
and per-task peripheral assignments.

## Hardware Pin Map

Full pin table in `docs/SPEC.md`. Key groups:

- **SPI1 (W5500):** PA5/SCK, PA6/MISO, PA7/MOSI, PA4/CS,
  PA3/INT (EXTI3), PA1/RST — DMA1_CH2 (RX), DMA1_CH3 (TX)
- **I2C1 (SSD1306):** PB6/SCL, PB7/SDA
- **USART2 (EasyComm):** PA2/TX, PA15/RX —
  DMA1_CH7 (TX), DMA1_CH6 (RX)
- **Steppers:** PA8/AZ STEP (TIM1_CH1), PA0/EL STEP (TIM2_CH1),
  PC14/AZ DIR, PA9/EL DIR, PB4/Motor EN
- **5-way nav:** PB1/UP, PB0/DOWN, PA11/LEFT, PA12/RIGHT,
  PC15/CENTER (all pull-up, active-low)
- **Endstops:** PA10/AZ Home, PB5/EL Home (pull-up, active-low)

Four 0 Ω resistors must be removed from the Nucleo board:
SB1/SB2 (free PC14/PC15 from 32 kHz oscillator) and SB16/SB18
(isolate PA5↔PB6 and PA6↔PB7 so SPI1 and I2C1 don't short
together). See `docs/SPEC.md` for details.

## Important Constraints

- `#![no_std]`, `#![no_main]` — no heap, no standard library
- STM32L432 has **no DMAMUX** — DMA channels are fixed per
  peripheral (SPI1→DMA1_CH2/CH3, USART2→DMA1_CH7(TX)/CH6(RX)).
  Wrong channels fail silently.
- `exti::InterruptHandler` is parameterized by the *interrupt
  type* (`interrupt::typelevel::EXTI3`), not the peripheral —
  unlike DMA/RNG handlers
- Task functions return `Result<SpawnToken, SpawnError>` — call
  `.unwrap()` before passing to `spawner.spawn()`
- `libm` is required for `sinf`/`cosf` in the OLED polar display
  (no hardware FPU trig)

## Cargo Features

- `test-spi` — enables a loop reading the W5500 chip version
  register for hardware debugging
