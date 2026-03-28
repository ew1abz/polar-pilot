# Polar Pilot — OLED Polar Display

## Overview

The SSD1306 128x64 OLED shows a polar diagram representing the antenna's
azimuth/elevation, with the current position indicated by a small airplane
icon. This is the primary local UI for the rotator controller.

## Display Layout

```text
┌──────────────────────────────────┐  128 x 64 px
│          N                       │
│      ·───┼───·   AZ: 045.0°     │  Status text area
│    /     |     \  EL: 30.0°     │  (right side,
│  W──────·┼·──────E              │   ~40 px wide)
│    \     |     /                 │
│      ·───┼───·                   │
│          S         ✈             │
│                   DHCP OK        │  Bottom-right:
└──────────────────────────────────┘  network status
```

### Polar Diagram (left region, ~88 x 64 px)

- **Center**: pixel (40, 32)
- **Concentric rings**: 2 rings at fixed radii representing elevation tiers
  - Outer ring (r=28 px) = 0° elevation (horizon)
  - Inner ring (r=14 px) = 45° elevation
  - Center dot = 90° elevation (zenith)
- **Crosshair lines**: thin 1 px lines through center, N/S/E/W
- **Cardinal labels**: N (top), S (bottom), E (right), W (left) — single
  character, placed just outside the outer ring
- **Projection**: azimuth maps to angle (0° = up/North, CW), elevation maps
  to radius (0° = outer ring, 90° = center) using linear interpolation:
  `r = outer_r * (1.0 - el / 90.0)`

### Antenna Position Marker

- A **5x5 pixel airplane bitmap** plotted at the computed (x, y) from the
  current azimuth and elevation
- Bitmap (5x5, 1-bit):

  ```text
  . . # . .
  . . # . .
  # # # # #
  . # # # .
  . # . # .
  ```

- The airplane is drawn with simple pixel-set calls (no rotation) — it always
  points "up" on screen (north)
- When the position is at the exact center (el=90°), draw the airplane at
  center

### Status Text (right region, ~40 x 64 px)

- **Line 1** (y=0): `AZ:` followed by azimuth in degrees, 1 decimal
  (e.g. `045.0`)
- **Line 2** (y=12): `EL:` followed by elevation in degrees, 1 decimal
  (e.g. `030.0`)
- **Line 3** (y=48): Network status — `DHCP OK`, `NO LINK`, or IP last octet
- Font: 6x8 built-in font from `embedded-graphics`

## Coordinate Mapping

Given azimuth `az` (0..360°) and elevation `el` (0..90°):

```text
angle_rad = az * PI / 180.0          // 0 = North (up), CW
r_px      = OUTER_R * (1.0 - el / 90.0)
x         = cx + r_px * sin(angle_rad)
y         = cy - r_px * cos(angle_rad)
```

Where `cx=40, cy=32, OUTER_R=28`.

Since `no_std` — use `libm::sinf` / `libm::cosf` (from the `libm` crate)
or a small integer lookup table for sin/cos.

## Update Rate

- Redraw the display at **4 Hz** (every 250 ms) from the display task
- Full-frame buffer approach: clear buffer, draw diagram, draw marker, draw
  text, flush to I2C
- Use `embedded-graphics` `MonoTextStyle` and `Framebuffer` or
  `ssd1306::mode::BufferedGraphicsMode`

## Dependencies

| Crate               | Purpose                                    |
|----------------------|--------------------------------------------|
| `ssd1306`            | SSD1306 I2C driver with buffered mode      |
| `embedded-graphics`  | Drawing primitives, fonts, image support   |
| `libm`               | `sinf`/`cosf` for polar coordinate math    |

## Task Integration

A new **display_task** joins the existing three async tasks:

```text
4. display_task — reads current az/el (from shared state), redraws
   the polar diagram on the SSD1306 every 250 ms via I2C1 (PB6/PB7)
```

Shared rotator state (azimuth, elevation, link status) is accessed via a
`Signal` or by reading from an `embassy_sync::watch::Watch`.

## Hardware

- **Display**: SSD1306 128x64, I2C address 0x3C
- **Bus**: I2C1 — PB6 (SCL), PB7 (SDA)
- **Speed**: 400 kHz (I2C fast mode)

## Rendering Constraints

- No heap — frame buffer is a static 128x64x1 = 1024-byte array
- All drawing uses integer arithmetic except the sin/cos for position mapping
- Total I2C transfer per frame: ~1 KB at 400 kHz = ~20 ms — fits comfortably
  in the 250 ms budget
