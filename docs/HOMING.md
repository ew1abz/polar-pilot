# Polar Pilot — Homing Procedure

## Purpose

Homing establishes the zero-degree reference for both axes using active-low
endstop switches. Until homing completes, reported positions are meaningless
and `GoTo` commands are rejected. The `Phase` field in `RotatorState` signals
the current state to all consumers (display, rotctld, easycom).

## When Homing Runs

| Trigger | Description |
|---------|-------------|
| **Power-on** | Runs automatically before any command is accepted |
| **Long-press CENTER** (>500 ms) | Operator-initiated re-home from `key_task` |

Re-homing sends a dedicated `RotatorCmd::Home` (to be added alongside
`GoTo`/`Stop`). `motor_task` drops any in-progress slew and restarts the
sequence.

## Phase Enum

```rust
pub enum Phase {
    Homing,   // homing in progress — commands queued but not executed
    Running,  // homed and operational
}
```

Published in every `RotatorState` send so all tasks can react.

## Axis Sequence

Axes are homed **serially**: AZ first, then EL. Both axes use identical logic.

```text
Phase::Homing published ──────────────────────────────────────────────────►
                                                            Phase::Running
  ┌──────────────┐   ┌──────────────┐   ┌──────────────┐
  │  Pre-check   │──►│   Approach   │──►│    Settle    │
  │  (back-off   │   │  (seek home) │   │  zero pos.   │
  │   if stuck)  │   │              │   │  disable PWM │
  └──────────────┘   └──────────────┘   └──────────────┘
        │ stuck?             │ limit?
        ▼                    ▼
   HomingError          HomingError
```

## Algorithm

### 1. Pre-check

Sample the endstop. If already asserted (switch closed, pin low):

- Drive the axis in the **positive direction** for a fixed back-off distance
  (10° equivalent steps) at homing speed.
- Re-sample the endstop.
- If still asserted → fault: `HomingError::EndStopStuck`. Disable motor,
  publish `Phase::Homing` with error, loop forever displaying the fault.

### 2. Approach

Drive the axis in the **negative direction** (toward home) at homing speed:

- Each `MOTOR_TICK` interval: sample the endstop, publish current state with
  `Phase::Homing`.
- When the endstop asserts → go to Settle.
- Track accumulated travel from the start position. If it exceeds
  `MAX_HOMING_TRAVEL` (AZ: 380°, EL: 100°) without the switch asserting →
  fault: `HomingError::LimitExceeded`.

### 3. Settle

- Stop PWM immediately.
- Set `current_az` / `current_el` to `0.0`.
- Set `target_az` / `target_el` to `0.0`.
- Disable motor driver (`motor_en` high).
- Proceed to the next axis (or transition to `Phase::Running` after EL).

## Parameters

| Parameter | AZ | EL | Notes |
|-----------|----|----|-------|
| Direction to home | negative (low) | negative (low) | both axes home toward zero |
| Back-off distance | 10° steps | 10° steps | at homing speed |
| Homing PWM frequency | `STEP_HZ` | `STEP_HZ` | same as slewing |
| Max travel limit | 380° | 100° | abort if exceeded |
| Endstop pin | PA10 (active-low, pull-up) | PB5 (active-low, pull-up) | |

## Error Handling

| Error | Cause | `Phase::Fault` payload |
| --- | --- | --- |
| Endstop stuck | Switch closed before approach, still closed after 3 s back-off | `"AZ endstop stuck"` / `"EL endstop stuck"` |
| Travel limit exceeded | `MAX_HOMING_TRAVEL` elapsed without switch asserting | `"AZ travel limit"` / `"EL travel limit"` |

On any fault:

1. Motor driver disabled (`motor_en` high), PWM stopped.
2. `Phase::Fault(msg)` published to the `STATE` watch — all consumers react
   immediately without polling.
3. **OLED**: polar chart replaced by a full-screen fault screen:

   ```text
   !! FAULT !!
   AZ stuck
   ```

4. **rotctld TCP**: `get_pos`, `set_pos`, and `stop` all respond with:

   ```text
   FAULT: AZ stuck
   RPRT -9
   ```

   `RPRT -9` is Hamlib's "command rejected" code; the tracking client will
   surface a connection error rather than silently accepting bad data.

5. `motor_task` loops forever sleeping — fault is unrecoverable without a
   power cycle.

`key_task` already ignores all input when `phase != Phase::Running`, so
button presses during a fault are silently dropped.

## Bounce Suppression

Embassy GPIO inputs use internal pull-ups. The endstop switches are mechanical;
a single `is_low()` sample per `MOTOR_TICK` is sufficient at the default tick
rate (10 ms) because mechanical bounce settles within ~1–2 ms. No additional
debounce filter is required for the stop condition.

## Key_task Integration

`key_task` sends `RotatorCmd::Home` on long-press CENTER. While
`Phase::Homing`, `motor_task` ignores `GoTo` commands (drains the channel
without acting). `Stop` is always honoured.

## Comparison with Rotator Project

The `rust-l432-rotator` project uses the `stepper-motion` crate with typed
motor states and a blocking step loop. This project uses Embassy async PWM
(TIM1/TIM2) with `Timer::after(MOTOR_TICK).await` between endstop samples,
achieving the same algorithm without blocking the executor. The phase
information previously encoded in Rust types (`Homed`, `Pointing`, `Homing`)
is represented here as the `Phase` field in the published `RotatorState`.
