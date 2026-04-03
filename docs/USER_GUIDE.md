# Polar Pilot — User Guide

## First Power-On

1. Connect the rotator controller to your network via Ethernet.
2. Apply power. The OLED shows a splash screen for 2 seconds, then homing begins automatically.
3. Both axes drive toward their home switches. The display shows **Homing** until both axes reach zero.
4. Once homing completes, the display shows **No IP** (waiting for DHCP) or **Idle** (IP assigned).
5. The IP address appears on the bottom two lines of the display, e.g.:

   ```text
   192.168.
   1.200
   ```

If the device does not receive a DHCP address within 2 minutes, it falls back
to a static IP (`192.168.1.200` by default — ask your installer if this was
changed).

---

## Connecting from gpredict / rotctld

Point rotctld at the device IP on port **4533**:

```bash
rotctld -m 204 -r 192.168.1.200:4533
```

Then connect gpredict to that rotctld instance as usual.

---

## Manual Control (5-Way Button)

| Button | Action                                        |
|--------|-----------------------------------------------|
| UP     | Jog azimuth clockwise (hold to keep moving)   |
| DOWN   | Jog azimuth counter-clockwise                 |
| RIGHT  | Jog elevation up                              |
| LEFT   | Jog elevation down                            |
| CENTER | Park — move both axes to AZ 0° / EL 0°       |

Buttons are ignored during homing. Normal operation resumes automatically
when homing finishes.

---

## Soft Travel Limits

Soft limits restrict where the rotator will move in response to commands
from rotctld or EasyComm. They do **not** affect manual button jogging.
Limits reset to the full range (AZ 0–360°, EL 0–180°) on every power cycle.

### Why use soft limits?

- Protect cabling that cannot reach certain positions
- Restrict elevation to your antenna's useful range (e.g. EL 5–90°)
- Keep azimuth within a sector if your installation is obstructed

### Setting limits via TCP (rotctld)

Connect with netcat or any TCP client:

```bash
nc 192.168.1.200 4533
```

**Get current limits:**

```text
l
```

Response:

```text
0.0        ← az_min
360.0      ← az_max
0.0        ← el_min
180.0      ← el_max
RPRT 0
```

**Set limits:**

```text
L <az_min> <az_max> <el_min> <el_max>
```

Examples:

```bash
# Restrict azimuth to a 180° sector (east hemisphere only)
L 90 270 0 180

# Set minimum elevation to 5° (ignore signals below horizon clutter)
L 0 360 5 180

# Reset to full range
L 0 360 0 180
```

Response: `RPRT 0` on success, `RPRT -1` on bad arguments.

### Setting limits via EasyComm (serial / USB)

Connect at **9600 baud** to the USB serial port (usually `/dev/ttyACM0` on
Linux, `COMx` on Windows).

**Get current limits:**

```text
LM
```

Response:

```text
LM 0.0 360.0 0.0 180.0
```

**Set limits:**

```text
LM <az_min> <az_max> <el_min> <el_max>
```

Examples:

```text
LM 90 270 0 180
LM 0 360 5 180
LM 0 360 0 180
```

No response is sent on success. Invalid arguments are silently ignored
(limits remain unchanged).

---

## Fault Handling

If a homing fault occurs (endstop stuck or travel limit exceeded), the display
shows:

```text
FAULT
AZ endstop stuck
Power cycle
to reset
```

All rotctld/EasyComm commands respond with `FAULT: <message>` while a fault
is active. **Power cycle the controller to recover.** Check the endstop
switches if the fault repeats.

---

## EasyComm II Quick Reference

| Command                        | Description                        |
|--------------------------------|------------------------------------|
| `AZ`                           | Query current AZ/EL position       |
| `AZ<degrees> EL<degrees>`      | Command a position                 |
| `SA` / `SE` / `SA SE`          | Stop azimuth / elevation / both    |
| `LM`                           | Get soft limits                    |
| `LM az_min az_max el_min el_max` | Set soft limits                  |
| `VE`                           | Query firmware version             |
