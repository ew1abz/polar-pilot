# Polar Pilot — Task Architecture

## Overview

The firmware runs as a set of cooperative async tasks on the Embassy executor.
All tasks share a single thread (Cortex-M4 has no OS) and yield at `.await`
points. Communication between tasks uses lock-free Embassy synchronization
primitives — no mutexes, no critical sections in application code.

## Shared State

A single `RotatorState` struct is the central coordination point:

```
RotatorState
├── target_az:  f32          // commanded azimuth   (0..360)
├── target_el:  f32          // commanded elevation  (0..90)
├── current_az: f32          // actual azimuth from step count
├── current_el: f32          // actual elevation from step count
├── moving:     bool         // true while motors are running
└── link_up:    bool         // Ethernet link status
```

**Primitive**: `embassy_sync::watch::Watch<CriticalSectionRawMutex, RotatorState, 4>`

- Writers call `watch.sender().send(state)` — non-blocking, overwrites
- Readers call `watch.receiver().changed().await` — wakes on new value
- 4 receivers: motor, display, rotctld, easycom

This is a single-producer (motor task owns the canonical position),
multi-consumer broadcast. Command tasks (rotctld, easycom, keys) write
target values; the motor task reads targets and updates current position.

### Command Channel

Target position commands from multiple sources funnel through a single
channel:

```
embassy_sync::channel::Channel<CriticalSectionRawMutex, RotatorCmd, 4>
```

```
enum RotatorCmd {
    GoTo { az: f32, el: f32 },
    Stop,
}
```

Producers: rotctld_task, easycom_task, key_task (all send commands).
Consumer: motor_task (receives and executes).

Last command wins — if rotctld sends `GoTo` while keys are active, the
newest command takes effect immediately.

## Task Map

```
                       ┌──────────────┐
                       │     main     │
                       │  (init only) │
                       └──────┬───────┘
                              │ spawns all tasks
          ┌──────────┬────────┼─────────┬─────────┐
          ▼          ▼        ▼         ▼         ▼
     ┌─────────┐ ┌───────┐ ┌──────┐ ┌───────┐ ┌───────┐
     │ motor   │ │displ. │ │ key  │ │rotctld│ │easycom│
     │         │ │       │ │      │ │       │ │       │
     │ TIM1/2  │ │ I2C1  │ │ GPIO │ │ TCP   │ │USART2 │
     │ GPIO    │ │SSD1306│ │ EXTI │ │ :4533 │ │serial │
     └─────────┘ └───────┘ └──────┘ └───────┘ └───────┘
          │          ▲        │         │         │
          │          │        │         │         │
          ▼          │        ▼         ▼         ▼
     ┌─────────┐     │   ┌──────────────────────────┐
     │  Watch  │─────┘   │     Command Channel      │
     │ (state) │◄────────│     (RotatorCmd)         │
     └─────────┘         └──────────────────────────┘

     Also spawned (no app-level interaction):
     ┌───────────────┐  ┌──────────────┐  ┌──────────────┐
     │ ethernet_task │  │   net_task   │  │   led_task   │
     │ W5500 Runner  │  │ Stack Runner │  │  heartbeat   │
     └───────────────┘  └──────────────┘  └──────────────┘
```

## Task Descriptions

### 1. main (init only)

- Configures clocks (80 MHz HSI+PLL)
- Initializes all peripherals: SPI1, I2C1, USART2, TIM1, TIM2, GPIOs
- Creates W5500 driver, embassy-net stack, shared state, command channel
- Spawns all tasks, then returns (does not loop)

**Peripherals consumed**: all — parceled out to tasks via ownership transfer.

### 2. ethernet_task

- Runs the `embassy-net-wiznet` W5500 Runner
- Pumps SPI frames between the W5500 and the network stack
- No interaction with application state

**Peripherals**: SPI1 (PA4–PA7, DMA1_CH2/CH3), W5500 INT (PA3), W5500 RST (PA1)

### 3. net_task

- Runs `embassy_net::Stack::run`
- Handles DHCP, ARP, ICMP, TCP timers
- No interaction with application state

**Peripherals**: none directly (operates on the Stack)

### 4. motor_task

**Purpose**: Translate target position into stepper motion.

- Receives `RotatorCmd` from the command channel
- Computes step count delta from current to target position
- Configures TIM1 (AZ) and TIM2 (EL) PWM frequency for ramp-up/cruise/ramp-down
- Sets direction pins (PC14 for AZ, PA9 for EL)
- Counts steps via timer update interrupts or elapsed-time estimation
- Updates `current_az` / `current_el` in the Watch after each step batch
- Checks endstop inputs (PA10 AZ home, PB5 EL home) and halts if triggered
- Controls motor enable pin (PB4, active-low)

**Peripherals**: TIM1 (PA8), TIM2 (PA0), GPIO out (PC14, PA9, PB4),
GPIO in (PA10, PB5)

**Loop cadence**: wakes on command channel receive or periodic 10 ms tick
for step counting and ramp updates.

### 5. display_task

**Purpose**: Render the polar diagram and status on the SSD1306 OLED.

- Receives state updates from the Watch
- Redraws at 4 Hz (250 ms tick): polar rings, crosshair, cardinal labels,
  airplane marker at current az/el, numeric readout, network status
- See [OLED_POLAR_DISPLAY.md](OLED_POLAR_DISPLAY.md) for rendering details

**Peripherals**: I2C1 (PB6 SCL, PB7 SDA)

**Loop cadence**: `Ticker::every(Duration::from_millis(250))` — redraws on
each tick using the latest Watch value.

### 6. key_task

**Purpose**: Read the 5-way navigation button and send commands.

- Scans 5 GPIO inputs: UP (PB1), DOWN (PB0), LEFT (PA11), RIGHT (PA12),
  CENTER (PC15)
- Debounce: 20 ms sample interval, require 2 consecutive identical reads
- Button actions:
  - **LEFT/RIGHT**: nudge target azimuth by +/- step increment
  - **UP/DOWN**: nudge target elevation by +/- step increment
  - **CENTER**: stop (send `RotatorCmd::Stop`)
  - **Long press CENTER** (>1 s): start homing sequence
- Sends `RotatorCmd::GoTo` or `RotatorCmd::Stop` to the command channel
- Optionally: hold-to-repeat (after 500 ms hold, repeat at 5 Hz)

**Peripherals**: GPIO input with pull-up (PB1, PB0, PA11, PA12, PC15)

**Loop cadence**: `Ticker::every(Duration::from_millis(20))` for debounce
sampling.

### 7. rotctld_task

**Purpose**: Accept TCP connections and speak the Hamlib rotctld protocol.

- Binds TCP socket on port 4533
- Accepts one connection at a time (single-socket W5500 allocation)
- Parses line-based commands: `p`, `P az el`, `S`, `q`, `_`, `\dump_state`
- `p` — reads current az/el from the Watch, responds with position
- `P az el` — sends `RotatorCmd::GoTo` to the command channel
- `S` — sends `RotatorCmd::Stop`
- Unrecognized commands return `RPRT -1`

**Peripherals**: none directly (uses embassy-net TCP socket)

**Loop cadence**: event-driven — blocks on `socket.read()`.

### 8. easycom_task

**Purpose**: Accept serial commands via the EasyComm II protocol on USART2.

- Reads from USART2 RX (PA15), writes to USART2 TX (PA2)
- Connected to ST-LINK virtual COM port (appears as /dev/ttyACM0 on host)
- Protocol: EasyComm II — line-based, `\n` terminated
- Key commands:
  - `AZxxx.x ELxxx.x` — set target position (sends `RotatorCmd::GoTo`)
  - `AZ EL` — query current position, responds `AZxxx.x ELxxx.x`
  - `SA SE` — stop azimuth/elevation (sends `RotatorCmd::Stop`)
  - `VE` — version query, responds with firmware identifier
- Shares the same command channel and Watch as rotctld

**Peripherals**: USART2 TX (PA2, AF7), USART2 RX (PA15, AF3)

**Loop cadence**: event-driven — blocks on USART2 DMA read.

### 9. led_task

- Toggles PB3 (onboard LED) at 1 Hz as a heartbeat
- No interaction with application state

**Peripherals**: GPIO out (PB3)

## Data Flow Summary

```
 [rotctld TCP]──┐
 [easycom UART]─┤  RotatorCmd   ┌────────────┐  Watch   ┌──────────┐
 [5-way keys]───┴──────────────►│ motor_task  ├─────────►│ display  │
                                │             │          │  (OLED)  │
                                │ TIM1+TIM2   │─────────►│          │
                                │ endstops    │   Watch  ├──────────┤
                                └─────────────┘─────────►│ rotctld  │
                                                  Watch  ├──────────┤
                                                ────────►│ easycom  │
                                                         └──────────┘
```

- **Commands flow in** from three sources via the Channel
- **State flows out** from motor_task via the Watch to all readers
- No task directly calls another — all coordination is through shared primitives

## Resource Budget

| Resource          | Usage                                          |
|-------------------|------------------------------------------------|
| Flash (256 KB)    | ~80–100 KB estimated with display + fonts      |
| RAM (64 KB)       | ~1 KB OLED framebuf + ~8 KB net buffers + stack|
| DMA channels      | DMA1_CH2 (SPI RX), DMA1_CH3 (SPI TX)           |
| Timers            | TIM1 (AZ step), TIM2 (EL step)                 |
| I2C               | I2C1 (SSD1306, 400 kHz)                        |
| SPI               | SPI1 (W5500, 1 MHz)                            |
| USART             | USART2 (EasyComm, 9600 baud)                   |
| Embassy tasks     | 9 concurrent (including stack runners)         |
