# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

Bare-metal Rust firmware for an antenna rotator controller (Hamlib rotctld protocol) on STM32L432KC Nucleo-32 + W5500 SPI Ethernet. Single-binary `no_std` project using the Embassy async framework.

## Build Commands

```bash
cargo build --release          # build firmware
cargo run --release            # build, flash via probe-rs, and show RTT logs
```

Requires: `rustup target add thumbv7em-none-eabihf` and `cargo install probe-rs-tools`.
There are no tests — this is a hardware-only embedded project.

## Architecture

Everything is in `src/main.rs` — intentionally a single-file example project.

**Three concurrent async tasks** run on the Embassy executor:
1. **main** — initializes peripherals, spawns other tasks, then runs the TCP server loop (accept on port 4533, parse rotctld commands)
2. **ethernet_task** — pumps the W5500 SPI driver (embassy-net-wiznet Runner)
3. **net_task** — drives the TCP/IP stack (DHCP, ARP, timers via embassy-net Runner)

**Key Embassy patterns used:**
- `bind_interrupts!` maps hardware IRQs (DMA1_CHANNEL2/3, EXTI3, RNG) to Embassy handlers
- `StaticCell` provides `'static` storage for task data (W5500 state, network stack resources)
- Device + Runner split: both the W5500 driver and embassy-net return a device handle + a runner future that must be spawned separately

**Hardware pin mapping** (all on CN3 header, pins A2–A7):
- SPI1: PA5/SCK, PA6/MISO, PA7/MOSI, PA4/CS — fixed DMA channels: TX=DMA1_CH3, RX=DMA1_CH2
- W5500 control: PA3/INT (EXTI3), PA2/RST

## Important Constraints

- `#![no_std]`, `#![no_main]` — no heap, no standard library
- STM32L432 has no DMAMUX — DMA channels are fixed per peripheral (SPI1 must use DMA1_CH2/CH3)
- The `exti::InterruptHandler` is parameterized by the *interrupt type* (`interrupt::typelevel::EXTI3`), not the peripheral — unlike DMA/RNG handlers which take peripherals
- Task functions return `Result<SpawnToken, SpawnError>` in embassy-executor 0.10 — call `.unwrap()` before passing to `spawner.spawn()`
- Rotator state is a stub (in-memory f32 azimuth/elevation) — no actual motor control
