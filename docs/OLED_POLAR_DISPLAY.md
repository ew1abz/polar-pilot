# Polar Pilot — OLED Polar Display

## Overview

The SSD1306 128×64 OLED shows a polar diagram representing the antenna's
azimuth and elevation, with the current position indicated by an airplane icon.
This is the primary local UI for the rotator controller.

## Display Layout

```text
┌──────────────────────────────────┐  128 × 64 px
│  N                               │
│    ┌─────────┐   AZ: 045        │
│    │  ·───·  │   EL:  30        │
│  W─┤──·+·──├─E                 │
│    │  ·───·  │                   │
│    └─────────┘   Idle            │
│  S               192.168.        │
│                  1.200           │
└──────────────────────────────────┘
```

Left panel (0–78 px): polar chart. Right panel (TX=80 px): status text.

## Polar Diagram

**Constants** (in `src/tasks/display.rs`):

| Constant  | Value | Meaning                        |
|-----------|-------|--------------------------------|
| `CX`      | 39    | Chart centre X                 |
| `CY`      | 32    | Chart centre Y                 |
| `R_OUTER` | 30    | Outer ring radius (horizon)    |
| `R_INNER` | 15    | Inner ring radius (45° elev.)  |

- **Outer ring** (r=30 px): horizon (EL = 0°)
- **Inner ring** (r=15 px): 45° elevation
- **Centre point**: zenith (EL = 90°)
- **Crosshair**: 1 px lines through centre, N–S and E–W
- **Cardinal labels**: `N` (0, 7), `S` (0, 57), `E` (CX+R+3, CY+3),
  `W` (CX−R−9, CY+3). N and S are on the left margin; E and W flank the ring.

## Coordinate Mapping

```text
angle_rad = az × π / 180          // 0 = North (up), clockwise
r         = R_OUTER × (1 − el / 90.0)
x         = CX + r × sin(angle_rad)
y         = CY − r × cos(angle_rad)
```

EL > 90° produces negative `r`, which naturally places the dot on the opposite
azimuth at the mirrored radius — no explicit fold-over needed.

`libm::sinf` / `libm::cosf` are used (no hardware trig on Cortex-M4 without FPU
intrinsics in `no_std`).

## Antenna Position Marker

17-pixel airplane icon: cross arms (±3 in one axis) plus 3×3 filled centre:

```text
      ·         (0,−3)
      ·         (0,−2)
  · · # # # · · (−3..+3, 0) + (−1..+1, −1/0/+1)
      ·         (0,+2)
      ·         (0,+3)
```

Pixel offsets (dx, dy) relative to computed (x, y):

```rust
const PLANE: [(i32, i32); 17] = [
                      (0, -3), (0, -2),
    (-3, 0), (-2, 0),
    (-1, -1), (0, -1), (1, -1),
    (-1,  0), (0,  0), (1,  0),
    (-1,  1), (0,  1), (1,  1),
               (2, 0), (3, 0),
                      (0,  2), (0,  3),
];
```

## Status Panel (right, TX = 80 px)

| Row  | y   | Content                              |
|------|-----|--------------------------------------|
| 1    | 10  | `AZ:` + current azimuth as integer   |
| 2    | 22  | `EL:` + current elevation as integer |
| 3    | 40  | Status string (see below)            |
| 4    | 52  | IP first two octets (`A.B.`)         |
| 5    | 62  | IP last two octets (`C.D`)           |

Font: `FONT_6X10` from `embedded-graphics`.

### Status String

| `Phase`        | Condition          | Displayed   |
|----------------|--------------------|-------------|
| `Homing`       | any                | `Homing`    |
| `Running`      | moving             | `Moving`    |
| `Running`      | IP assigned        | `Idle`      |
| `Running`      | no IP yet          | `No IP`     |
| `Fault(msg)`   | —                  | fault screen (see below) |

IP is queried directly from the Stack via `stack.config_v4()` — not via
`RotatorState.link_up` (which is always false).

### IP Address Display

When DHCP has completed, the IP is split across two rows:
```
192.168.
1.200
```
While waiting for DHCP (or no IP):
```
---.---.
---.---
```

## Fault Screen

When `Phase::Fault(msg)` is set, the entire panel is replaced:

```text
FAULT
<message>
Power cycle
to reset
```

Text positions: y = 12, 32, 50, 60. The display loops on this screen until
power is cycled — the fault is unrecoverable.

## Splash Screen

Shown for 2 seconds at boot before homing begins:

- Left 64×64 px: Rust logo (`src/rust.raw`, 1-bpp raw bitmap, `include_bytes!`)
- Right half:
  - `"Polar"` at (74, 24)
  - `"Pilot"` at (74, 38)
  - `"v<CARGO_PKG_VERSION>"` at (70, 56)

## Update Rate

4 Hz (250 ms `Ticker`). Full-buffer redraw each tick:
clear → draw chart → draw marker → draw text → `display.flush()`.

The blocking I2C flush at 100 kHz takes ~92 ms per frame. `motor_task`
compensates via its elapsed-time step accumulator (`as_micros()` + fractional
carry), so virtual position tracks physical motion regardless of display delays.

## Hardware

| Item     | Detail                          |
|----------|---------------------------------|
| Display  | SSD1306 128×64, I2C addr 0x3C   |
| Bus      | I2C1 — PB6 (SCL), PB7 (SDA)   |
| Speed    | 100 kHz (blocking)              |
| Buffer   | 1024-byte static framebuffer    |
