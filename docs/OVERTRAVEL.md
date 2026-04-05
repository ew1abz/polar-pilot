# 1.5 Rotation (540° Overtravel)

> **Status: Not implemented.** This is a planned feature. See the Roadmap in
> [README.md](../README.md).

## Overview

Adding overtravel (540° rotation) eliminates the "dead zone" at 0°/360° north.
High-end commercial and amateur rotators use this approach to avoid forced
unwinds during satellite passes that cross the north mark.

With a standard 360° rotator, a satellite crossing 0° north forces an
immediate full-circle unwind. With a 540° rotator (0° to 540°), that same
pass can be tracked from 350° through 0° all the way to 180° before hitting a
physical limit — cutting required unwinds significantly.

## What Needs to Change

### 1. Mechanical

- **Cable service loop** — the coax must have enough slack to handle 540° of
  twist without tensioning connectors or rubbing against sharp edges.
- **Soft limits** — azimuth range extended from `0–360°` to `0–540°`.

### 2. Firmware Logic (The Overlap)

The azimuth range maps to physical angles as follows:

| Range | Physical angle |
|-------|---------------|
| 0° – 180° | Primary range |
| 180° – 360° | Secondary range |
| 360° – 540° | Overlap (same as 0° – 180°) |

`motor_task` needs to track raw stepper position across the 360° boundary
without wrapping, and the soft-limit enforcement must accept `max_az` up to
540°.

### 3. Host Software (Hamlib / rotctld)

gpredict does not know the rotator can go past 360° without explicit
configuration. Pass the extended range to rotctld with `-m` / `-M`:

```bash
rotctld -m 0 -M 540 -model [YOUR_MODEL_ID]
```
