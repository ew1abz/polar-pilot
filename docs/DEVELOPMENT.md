# Polar Pilot — Development Notes

## Prerequisites

```bash
rustup target add thumbv7em-none-eabihf
cargo install probe-rs-tools       # only needed for flashing
```

Python 3.10+ is required for the protocol test suite:

```bash
python3 -m pip install pytest pytest-timeout   # stub + rotctld live tests
python3 -m pip install pyserial                # EasyComm live tests only
```

## Build

```bash
cargo build --release
```

## Flash + RTT Log

```bash
cargo run --release
```

Configured via `.cargo/config.toml` to call `probe-rs run --chip STM32L432KCUx`.

## Hardware Test Binaries

Standalone binaries in `src/bin/` exercise individual peripherals in isolation.
Flash them the same way as the main firmware:

```bash
cargo run --release --bin w5500_test    # W5500 chip version + link status
cargo run --release --bin button_test   # 5-way nav button read-out
cargo run --release --bin motor_test    # stepper sweep
cargo run --release --bin oled_test     # OLED polar chart render
cargo run --release --bin endstop_test  # endstop state over RTT
```

## Testing

### Overview

There are two testing layers:

| Layer | Runs in | Requires hardware |
|---|---|---|
| Protocol tests (`tests/test_protocol.py`) | CI and locally | No |
| Live hardware tests (`tests/test_live.py`) | Manually, pre-release | Yes |

The protocol tests run against an in-process Python TCP stub (`tests/stub_server.py`)
that mirrors the firmware's rotctld dispatch table.  The stub's state is a plain
Python object set directly in each test fixture — no sleeping, no timing, no motor
physics.

### Running Protocol Tests (Stub)

```bash
python3 -m pytest                         # uses pytest.ini defaults (excludes live tests)
python3 -m pytest tests/test_protocol.py  # explicit
```

52 tests covering all commands, argument errors, protocol robustness
(pipelining, split TCP segments, CRLF line endings, oversized lines).

### Running Live Hardware Tests

Set `ROTATOR_HOST` to the device's IP address (DHCP-assigned or static fallback
`192.168.1.200`):

The default `pytest.ini` excludes live tests with `-m "not live"`.  Pass
`-m live` (or `-m "live or slow"` to include motor-movement tests) to
override it:

```bash
# rotctld (TCP)
ROTATOR_HOST=192.168.1.42 python3 -m pytest tests/test_live.py -v --timeout=60 -m live

# EasyComm II (serial) -- requires pyserial and a USB-serial adapter on USART2
ROTATOR_PORT=/dev/ttyUSB0 python3 -m pytest tests/test_easycom.py -v --timeout=60 -m live

# Include slow motor-movement tests
ROTATOR_HOST=192.168.1.42 python3 -m pytest tests/test_live.py -v --timeout=60 -m "live or slow"

# Both together
ROTATOR_HOST=192.168.1.42 ROTATOR_PORT=/dev/ttyUSB0 python3 -m pytest tests/ -v --timeout=60 -m "live or slow"

# All 87 tests (stub + live + slow) -- overrides the default -m "not live" in pytest.ini
ROTATOR_HOST=192.168.1.42 ROTATOR_PORT=/dev/ttyUSB0 python3 -m pytest tests/ -v --timeout=120 --override-ini="addopts="

# List all tests without running them
python3 -m pytest tests/ --collect-only -q --override-ini="addopts="
```

Live tests cover DHCP/TCP connectivity, motor convergence to target, stop
response, reset/park, soft limit enforcement, and two concurrent clients.

### Test File Layout

```text
tests/
  stub_server.py      in-process TCP stub + RotatorState
  conftest.py         conn_stub (per-test clean state) and conn_live fixtures
  test_protocol.py    52 protocol tests against stub
  test_live.py        hardware tests, skipped unless ROTATOR_HOST is set
pytest.ini            marks live/slow; excludes live tests from default run
```

### Notes on Protocol Behaviour Under Test

- **Unknown commands** return `RPRT -8` (RIG_ENIMPL).
- **Invalid arguments** (wrong count, non-numeric) return `RPRT -1` (RIG_EINVAL).
- **`w`/`\send_cmd`** returns `RPRT -1` — no backend pass-through target exists.
- **Empty or whitespace-only lines** return `RPRT -8` and do not crash the server.
- **`P` with out-of-limit coordinates** is silently clamped by `motor_task` —
  the TCP layer returns `RPRT 0` and the motor stops at the limit.
- **Line buffer** is 128 bytes; lines longer than this close the connection.

## Crate Dependencies

| Crate                | Purpose                                |
|----------------------|----------------------------------------|
| `embassy-executor`   | Async task executor                    |
| `embassy-stm32`      | HAL for STM32L4 (SPI, I2C, GPIO, …)   |
| `embassy-time`       | Timekeeping (TIM15 driver, Ticker)     |
| `embassy-sync`       | Watch + Channel + Mutex for inter-task comms |
| `embassy-net`        | TCP/IP stack (TCP, DHCPv4)             |
| `embassy-net-wiznet` | W5500 driver for embassy-net           |
| `embedded-hal-bus`   | SPI ExclusiveDevice wrapper            |
| `embedded-io-async`  | Async I/O traits (Write::write_all)    |
| `ssd1306`            | SSD1306 OLED driver                    |
| `embedded-graphics`  | 2D drawing primitives for OLED         |
| `heapless`           | Fixed-capacity String/Vec (no alloc)   |
| `libm`               | Software float math (sinf, cosf)       |
| `defmt` + `defmt-rtt`| Logging via RTT (non-blocking)         |
| `panic-reset`        | Reset on panic — safe for standalone   |
| `static_cell`        | One-time `'static` init for task data  |
| `cortex-m` / `cortex-m-rt` | Runtime + vector table           |

## Binary Size

Release build (`opt-level = "s"`, LTO):

| Configuration                        | .text (bytes) | Delta               |
|--------------------------------------|---------------|---------------------|
| Minimal (TCP + static IP, no defmt)  | 42,312        | baseline            |
| Minimal (TCP + static IP, defmt)     | 66,384        | +24,072 (+56.9%)    |
| + DHCP                               | 75,672        | +33,360 (+78.8%)    |
| + DHCP + ICMP echo reply             | 78,112        | +35,800 (+84.6%)    |

STM32L432KC has 256 KB flash. The `minimal` branch has static-IP-only builds
for reference.

## Project Structure

```text
.
├── .cargo/config.toml              # target, runner, DEFMT_LOG
├── .github/workflows/ci.yml        # CI: build + protocol tests
├── build.rs                        # memory.x linker script setup
├── memory.x                        # Flash/RAM layout for STM32L432
├── Cargo.toml
├── pytest.ini
├── tests/                          # rotctld protocol test suite (Python)
├── src/
│   ├── main.rs                     # peripheral init, task spawning
│   ├── types.rs                    # RotatorState, RotatorCmd, Phase, SoftLimits, statics
│   ├── util.rs                     # parse_f32, parse_f32_bytes, line helpers
│   ├── tasks/
│   │   ├── motor.rs                # motor_task — homing, slewing, endstops
│   │   ├── display.rs              # display_task — SSD1306 polar chart
│   │   ├── keys.rs                 # key_task — 5-way button
│   │   ├── rotctld.rs              # rotctld_task — TCP :4533
│   │   ├── easycom.rs              # easycom_task — USART2 EasyComm II
│   │   └── net.rs                  # ethernet_task, net_task, led_task, dhcp_watchdog_task
│   └── bin/                        # standalone hardware test binaries
│       ├── w5500_test.rs
│       ├── button_test.rs
│       ├── motor_test.rs
│       ├── oled_test.rs
│       └── endstop_test.rs
├── docs/
│   ├── SPEC.md                     # hardware spec and protocol reference
│   ├── DEVELOPMENT.md              # this file
│   ├── TASK_ARCHITECTURE.md        # task data flow and peripheral map
│   ├── OLED_POLAR_DISPLAY.md       # OLED rendering details
│   ├── HOMING.md                   # homing algorithm and fault handling
│   ├── MANUAL_CONTROL.md           # 5-way button behaviour
│   ├── MIGRATION_NOTES.md          # comparison with rust-l432-rotator
│   ├── TODO.md                     # known gaps and planned work
│   └── EEPROM_EMULATION.md         # (future) persistent configuration
```

## Known Issues & Workarounds

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
