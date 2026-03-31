# Polar Pilot — Task Architecture

## Overview

The firmware runs as a set of cooperative async tasks on the Embassy executor.
All tasks share a single thread (Cortex-M4 has no OS) and yield at `.await`
points. Communication between tasks uses lock-free Embassy synchronization
primitives.

## Shared State

### `STATE` — Watch

`Watch<CriticalSectionRawMutex, RotatorState, 4>`

Single-producer (motor_task), multi-consumer broadcast of current state.

```rust
struct RotatorState {
    target_az:  f32,   // commanded azimuth   (degrees)
    target_el:  f32,   // commanded elevation (degrees)
    current_az: f32,   // virtual position from step accumulator
    current_el: f32,   // virtual position from step accumulator
    moving:     bool,  // true while any axis is slewing
    link_up:    bool,  // (reserved, always false — IP read directly from Stack)
    phase:      Phase, // Homing | Running | Fault(&'static str)
}
```

Writers call `STATE.sender().send(state)` — non-blocking, overwrites.
Readers call `STATE.try_get()` — returns the latest value without blocking.

### `CMD` — Channel

`Channel<CriticalSectionRawMutex, RotatorCmd, 4>`

Multi-producer command funnel into motor_task.

```rust
enum RotatorCmd {
    GoTo { az: f32, el: f32 },
    Stop,
}
```

Producers: rotctld_task, easycom_task, key_task.
Consumer: motor_task (clamped to LIMITS before applying).

### `LIMITS` — Mutex

`Mutex<CriticalSectionRawMutex, Cell<SoftLimits>>`

Operator-configurable travel limits. Read by motor_task on every GoTo;
written by rotctld_task (`L`) and easycom_task (`LM`). Defaults to
AZ 0–360°, EL 0–180°. Resets to defaults on power cycle.

## Task Map

```text
                       ┌──────────────┐
                       │     main     │
                       │  (init only) │
                       └──────┬───────┘
                              │ spawns all tasks
      ┌──────┬──────┬─────────┼──────┬────────┬──────┬──────┐
      ▼      ▼      ▼         ▼      ▼        ▼      ▼      ▼
  ┌──────┐ ┌────┐ ┌──────┐ ┌──────┐ ┌──────┐ ┌────┐ ┌────┐ ┌────┐
  │motor │ │disp│ │ key  │ │rotcld│ │easyco│ │eth │ │net │ │led │
  │ task │ │ lay│ │ task │ │  task│ │m task│ │task│ │task│ │task│
  └──────┘ └────┘ └──────┘ └──────┘ └──────┘ └────┘ └────┘ └────┘
      │        ▲      │         │        │
      │ STATE  │      │   CMD   │        │   LIMITS
      ▼        │      ▼         ▼        ▼
  ┌───────┐    │  ┌──────────────────────────┐
  │ Watch │────┘  │     Command Channel      │
  │(STATE)│◄──────│       (RotatorCmd)       │
  └───────┘       └──────────────────────────┘

  Also spawned:
  ┌─────────────────┐
  │ dhcp_watchdog   │  waits 120 s, then sets static IP via Stack
  └─────────────────┘
```

## Task Descriptions

### 1. main (init only)

- Configures clocks (80 MHz HSI+PLL)
- Initializes all peripherals: SPI1, I2C1, USART2, TIM1, TIM2, GPIOs, RNG
- Creates W5500 driver, embassy-net stack
- Publishes initial `RotatorState::default()` to STATE
- Spawns all tasks, then returns (idles)

**Peripherals consumed**: all — parceled out to tasks via ownership transfer.

### 2. ethernet_task

- Runs the `embassy-net-wiznet` W5500 Runner
- Pumps SPI frames between the W5500 and the network stack

**Peripherals**: SPI1 (PA4–PA7, DMA1_CH2/CH3), W5500 INT (PA3), W5500 RST (PA1)

### 3. net_task

- Runs `embassy_net::Stack::run`
- Handles DHCP, ARP, ICMP, TCP timers

**Peripherals**: none directly (operates on the Stack)

### 4. dhcp_watchdog_task

- Calls `stack.wait_config_up()` with a 120 s timeout
- On timeout: calls `stack.set_config_v4(Static(...))` with the predefined
  fallback address (`192.168.1.200/24`)
- After resolving (either way), sleeps indefinitely

**Peripherals**: none (operates on the Stack)

### 5. motor_task

**Purpose**: homing, position tracking, stepper control.

**Homing (runs once at power-on):**

1. Publishes `Phase::Homing`
2. AZ axis: back-off if already on endstop (3 s timeout → `Fault`), then
   approach in negative direction until endstop asserts (95 s limit → `Fault`).
   Sets `current_az = 0`, `target_az = 0`.
3. EL axis: same sequence (28 s limit).
4. Publishes `Phase::Running`.

On any homing fault: disables motor driver, publishes
`Phase::Fault("AZ endstop stuck" | "AZ travel limit" | …)`, loops forever.

**Normal operation loop (1 ms tick):**

- Receives `RotatorCmd` from CMD channel (with 1 ms timeout)
- GoTo: clamps targets to LIMITS, then updates target_az/target_el
- Stop: sets targets to current position
- Advances virtual position each tick using step accumulator:
  `steps = floor(elapsed_us × STEP_HZ / 1_000_000)` — fractional remainder
  carries forward to prevent truncation drift
- Enables/disables TIM1 (AZ) and TIM2 (EL) PWM based on whether axis is moving
- Hardware endstop safety: if endstop asserts while moving toward home,
  zeroes position and stops PWM
- Software limits: AZ lower limit removed (allows wrap through north);
  AZ upper 360° and EL 0°/180° limits stop PWM and reset target
- Motor enable pin (PB4) driven low while moving, high when idle
- Publishes RotatorState every tick

**Motor parameters** (in `src/tasks/motor.rs`):

| Parameter      | Value           |
|----------------|-----------------|
| STEP_HZ        | 4000 Hz         |
| STEPS_PER_REV  | 200 (full step) |
| GEAR_RATIO     | 54:1            |
| AZ_MICROSTEPS  | 32              |
| EL_MICROSTEPS  | 32              |
| MOTOR_TICK     | 1 ms            |

**Peripherals**: TIM1 (PA8), TIM2 (PA0), GPIO out (PC14 AZ DIR, PA9 EL DIR,
PB4 motor EN), GPIO in (PA10 AZ home, PB5 EL home)

### 6. display_task

**Purpose**: render the polar diagram and status on the SSD1306 OLED.

- Receives `Stack` handle for direct IP/link-status queries via `config_v4()`
- Redraws at 4 Hz (250 ms Ticker)
- Fault screen: full-panel text (`FAULT` / message / `Power cycle` / `to reset`)
- Normal screen: polar rings, crosshair, cardinal labels (N/S/E/W),
  airplane marker at current az/el, AZ/EL readout, status, IP address
- IP shown split across two rows: `A.B.` / `C.D`, or `---.---.` / `---.---`
  while waiting for DHCP
- Status line: `Homing` / `Moving` / `Idle` / `No IP`

See [OLED_POLAR_DISPLAY.md](OLED_POLAR_DISPLAY.md) for rendering details.

**Peripherals**: I2C1 (PB6 SCL, PB7 SDA) — blocking 100 kHz

**Note**: blocking I2C flush (~92 ms per frame) is compensated by motor_task's
elapsed-time step accumulator — virtual position tracks physical motion
regardless of display task scheduling delays.

### 7. key_task

**Purpose**: read the 5-way navigation button and send commands.

- Scans 5 GPIO inputs: UP (PB1), DOWN (PB0), LEFT (PA11), RIGHT (PA12),
  CENTER (PC15)
- Debounce: 20 ms poll, require 2 consecutive identical reads
- Button actions:
  - **UP**: jog AZ positive (sentinel target +9999°, motor limits enforce stop)
  - **DOWN**: jog AZ negative (sentinel target −9999°)
  - **RIGHT**: jog EL positive (sentinel target +9999°)
  - **LEFT**: jog EL negative (sentinel target −9999°)
  - **CENTER press**: park both axes to AZ 0° / EL 0°
- Release any jog button → `RotatorCmd::Stop`
- Simultaneous AZ + EL buttons combined into one GoTo per tick
- All input ignored when `phase != Phase::Running`

**Peripherals**: GPIO input pull-up (PB1, PB0, PA11, PA12, PC15)

### 8. rotctld_task

**Purpose**: Hamlib rotctld TCP server.

- Binds TCP socket on port 4533; accepts one connection at a time
- 30 s inactivity timeout per connection
- Checks `Phase::Fault` on every command — returns `FAULT: <msg>\nRPRT -9\n`
- Supported commands: `p`/`\get_pos`, `P`/`\set_pos`, `S`/`\stop`,
  `l`/`\get_limits`, `L`/`\set_limits`, `q`/`\quit`, `_`/`\get_info`,
  `\dump_state`
- `\dump_state` reflects current LIMITS values

**Peripherals**: none (embassy-net TCP socket)

### 9. easycom_task

**Purpose**: EasyComm II serial interface.

- USART2 RX (PA15), TX (PA2) — 9600 baud, DMA
- Connected to ST-LINK VCP (appears as `/dev/ttyACM0`)
- Supported commands: `AZ`/`AZ<n> EL<n>`, `SA`/`SE`/`SA SE`, `VE`,
  `LM` (get limits), `LM <az_min> <az_max> <el_min> <el_max>` (set limits)

**Peripherals**: USART2 TX (PA2, AF7), USART2 RX (PA15, AF3),
DMA1_CH7 (TX), DMA1_CH6 (RX)

### 10. led_task

- Toggles PB3 (onboard LED) at 1 Hz as heartbeat
- `Ticker::every(Duration::from_secs(1))`

**Peripherals**: GPIO out (PB3)

## Data Flow Summary

```text
 [rotctld TCP]──┐
 [easycom UART]─┤  RotatorCmd      ┌─────────────┐  STATE   ┌──────────┐
 [5-way keys]───┴─────────────────►│ motor_task  ├─────────►│ display  │
                                   │             │          └──────────┘
                RotatorCmd         │ TIM1+TIM2   │  STATE   ┌──────────┐
  LIMITS ──────────────────────────│ endstops    ├─────────►│ rotctld  │
  (read on GoTo)                   └─────────────┘          └──────────┘
                                                   STATE   ┌──────────┐
                                                 ─────────►│ easycom  │
                                                           └──────────┘
```

- **Commands** flow in from three sources via CMD channel
- **State** flows out from motor_task via STATE watch to all readers
- **LIMITS** are written by rotctld/easycom, read by motor_task on GoTo
- **IP/link status** read directly from Stack by display_task (not via STATE)

## Resource Budget

| Resource          | Usage                                                  |
|-------------------|--------------------------------------------------------|
| Flash (256 KB)    | ~80–100 KB with display + networking + fonts           |
| RAM (64 KB)       | 1 KB OLED framebuf + ~8 KB net buffers + task stacks  |
| DMA channels      | DMA1_CH2 (SPI RX), CH3 (SPI TX), CH6 (UART RX), CH7 (UART TX) |
| Timers            | TIM1 (AZ step), TIM2 (EL step), TIM15 (Embassy time)  |
| I2C               | I2C1 (SSD1306, 100 kHz blocking)                       |
| SPI               | SPI1 (W5500, 1 MHz async)                              |
| USART             | USART2 (EasyComm, 9600 baud)                           |
| Embassy tasks     | 10 concurrent                                          |
