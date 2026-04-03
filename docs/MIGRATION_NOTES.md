# Comparison with rust-l432-rotator — Gaps & TODO

Reviewed: 2026-03-30

## Feature Comparison

| Feature | rust-l432-rotator | this project |
|---------|-------------------|--------------|
| Network | None (serial only) | W5500 Ethernet, TCP :4533, DHCP + static fallback |
| rotctld TCP | No | Yes |
| EasyComm II | Yes | Yes |
| OLED display | Table layout (POS/TGT rows, AZ/EL columns, large bold digits) | Polar chart with airplane marker |
| Homing | Non-blocking FSM | Async sequential |
| Homing fault display | Status line "ERR: AZ home" | Full-screen fault + "Power cycle" |
| Soft limits | No | Yes — runtime via TCP (`L`) and EasyComm (`LM`) |
| Manual jogging | Yes | Yes |
| Center button | Park to 0°/0° | Park to 0°/0° |
| Long-press CENTER re-home | Not implemented | Documented in HOMING.md but not implemented in code |
| Motor acceleration | Yes — trapezoidal ramp, 100°/sec², 25°/sec max | No — constant speed (full STEP_HZ immediately) |
| Step generation | TIM6/TIM7 one-pulse ISR (interval varies per step) | TIM1/TIM2 fixed-frequency PWM |
| Position tracking | `stepper-motion` crate internal counter | Software µs accumulator with fractional carry |
| LED state encoding | Blink pattern encodes state (solid/slow/medium/fast) | Fixed 1 Hz regardless of state |
| DHCP fallback | N/A | 120 s then static IP |
| Panic handler | panic-probe (halts without debugger) | panic-reset (safe standalone) |
| Runtime | HAL blocking + interrupt loop | Embassy async |

---

## What Needs Changing (priority order)

### 1. Motor acceleration — HIGH PRIORITY (mechanical safety)

**Problem:** Motors start and stop at full speed (STEP_HZ = 4000 Hz). On heavy
antenna loads this causes missed steps and mechanical shock.

**rust-l432-rotator approach:** `stepper-motion` crate with trapezoidal profile —
100°/sec² acceleration, 25°/sec cruise speed, smooth ramp-down.

**Options:**

- a) Implement ramp-up/ramp-down in `motor_task` by ramping PWM frequency
     (start at low freq, increase to STEP_HZ over N steps, reverse at end)
- b) Port `stepper-motion` into the Embassy async model
- c) At minimum: document the constant-speed limitation and test for missed steps

---

### 2. Long-press CENTER / `RotatorCmd::Home` — MEDIUM PRIORITY

**Problem:** `HOMING.md` documents "Long-press CENTER (>500 ms) triggers
re-homing" but `key_task` only detects a press edge and sends `GoTo {0, 0}`.
There is no `RotatorCmd::Home` variant and no way to trigger re-homing without
a power cycle.

**Changes needed:**

- Add `RotatorCmd::Home` variant to `types.rs`
- Implement long-press detection in `key_task` (hold timer, >500 ms threshold)
- Handle `RotatorCmd::Home` in `motor_task`: drop current move, restart homing FSM
- Either expose re-homing via rotctld (`H` command?) and/or EasyComm

---

### 3. EasyComm command coverage — DONE

Added GS-232A/B commands to `src/tasks/easycom.rs`:

- `C` — query both axes (responds `AZ<n> EL<n>`)
- `A<NNN>` — set azimuth only (accepts int or decimal)
- `E<NNN>` — set elevation only (accepts int or decimal)
- `W<az> <el>` — set both axes (GS-232B format)
- `?` — keep-alive (responds `\r`)

---

### 4. LED state encoding — LOW PRIORITY (usability)

**Problem:** `led_task` always blinks at 1 Hz. rust-l432-rotator uses blink
pattern to communicate state at a glance without looking at the OLED.

**Proposed mapping:**

| State | Pattern |
|-------|---------|
| Homing | Fast blink (200 ms) |
| Moving | Slow blink (500 ms) |
| Fault | Double-flash |
| Idle | 1 Hz (current) |

**File to change:** `src/tasks/net.rs` — `led_task` needs to read STATE.

---

### 5. Display table mode — LOW PRIORITY (subjective)

**Problem:** rust-l432-rotator uses a numeric table with 9×18 bold digits —
easier to read the exact position from across the room. The polar chart is
better for situational awareness but harder to read precise numbers.

**Option:** Add a second display mode toggled by a button combination,
or show both (split screen — polar left, numeric right, which is already
partly done with the right-panel text).

---

## Not Needed (already better in this project)

- Networking — this project adds W5500 TCP which rust-l432-rotator lacks entirely
- Soft limits — not in rust-l432-rotator at all
- DHCP fallback — not in rust-l432-rotator
- Standalone operation (panic-reset) — already fixed
- Polar chart UI — subjectively richer than table layout
