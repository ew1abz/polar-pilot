# Manual Control — 5-Way Navigation Button

## Overview

A 5-way navigation button provides direct manual jogging of the rotator axes.
Manual input works in parallel with the rotctld TCP and EasyComm II serial
interfaces — all sources drive the same command channel. Motors run only while
a direction button is held; releasing stops the axis.

## Hardware

- **5-way navigation button** — UP / DOWN / LEFT / RIGHT / CENTER.
  Active-low with internal pull-ups; debounced in firmware.

### GPIO Assignment

| Button | Pin  | Nucleo Label | Notes               |
|--------|------|--------------|---------------------|
| UP     | PB1  | D6 (CN3)     | pull-up, active-low |
| DOWN   | PB0  | D3 (CN3)     | pull-up, active-low |
| LEFT   | PA11 | D10 (CN3)    | pull-up, active-low |
| RIGHT  | PA12 | D2 (CN3)     | pull-up, active-low |
| CENTER | PC15 | D8 (CN3)     | pull-up, active-low |

### Button Mapping

| Button | Action                                                      |
|--------|-------------------------------------------------------------|
| UP     | Jog AZ positive (while held, stops at soft limit)           |
| DOWN   | Jog AZ negative (while held, stops at soft limit or 0°)     |
| RIGHT  | Jog EL positive (while held, stops at soft limit or 180°)   |
| LEFT   | Jog EL negative (while held, stops at soft limit or 0°)     |
| CENTER | Park: move both axes to AZ 0° / EL 0°                      |

## Behaviour

### Jogging (UP / DOWN / LEFT / RIGHT)

- On press: a `GoTo` is sent each poll tick with a sentinel target of ±9999°.
  Motor_task clamps the target to the current soft limits, so the axis runs
  until it reaches the limit and stops.
- On release: `RotatorCmd::Stop` is sent; axis stops immediately.
- Simultaneous AZ + EL buttons are combined into a single `GoTo` per tick —
  both axes move concurrently.
- Opposite directions on the same axis: `up` beats `down`; `right` beats `left`
  (evaluation order in the poll loop).

### Parking (CENTER)

- A single press edge sends `GoTo { az: 0.0, el: 0.0 }`.
- Both motors slew to 0°/0° and stop when they arrive.
- Any subsequent jog press overrides the park move.

### Parallel Operation

- Manual and serial inputs share the `CMD` channel — no mode switch required.
  Last command wins.

### Phase Gate

- All button input is ignored when `phase != Phase::Running` (during homing
  or a fault condition).
- Normal operation resumes automatically when `Phase::Running` is published
  by motor_task after homing completes.

## Firmware

### Module: `src/tasks/keys.rs`

`BtnState` tracks per-button debounce with a saturating counter
(`DEBOUNCE_SAMPLES = 2`). `update(pin_low)` returns `true` on any edge
(press or release) and keeps `pressed` reflecting the debounced state.

### Polling Loop

Runs every 20 ms (`Ticker::every(Duration::from_millis(20))`). Each tick:

1. Snapshot `was_jogging` (any jog button held last tick).
2. Update all five debounce counters.
3. Read `Phase` from `STATE`; skip the rest if not `Running`.
4. On CENTER press edge: send `GoTo { az: 0.0, el: 0.0 }`.
5. If any jog button is held: send combined `GoTo` with ±9999 sentinels
   on the active axis and `current_az`/`current_el` on the idle axis.
6. Else if `was_jogging`: send `Stop`.
