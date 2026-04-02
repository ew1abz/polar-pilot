"""
Live hardware tests for the Polar Pilot EasyComm II / GS-232 serial interface.

Requires a USB-serial adapter connected to the device's USART2 (PA2/TX, PA15/RX)
at 9600 8N1.  Set ROTATOR_PORT before running:

    ROTATOR_PORT=/dev/ttyUSB0 python3 -m pytest tests/test_easycom.py -v --timeout=60

Also requires pyserial:
    python3 -m pip install pyserial

All tests are marked ``live`` and are excluded from the default run.
"""

import os
import re
import time

import pytest

try:
    import serial
except ImportError:
    serial = None  # type: ignore


# ── fixtures ──────────────────────────────────────────────────────────────────

def _port() -> str:
    p = os.environ.get("ROTATOR_PORT", "")
    if not p:
        pytest.skip("ROTATOR_PORT not set")
    return p


@pytest.fixture(scope="module")
def ser():
    """Single serial connection shared across all tests in this module.

    Opening and closing the port for every test was causing the USART DMA
    to accumulate errors, making subsequent commands unreliable.  One
    persistent connection avoids that churn entirely.
    """
    if serial is None:
        pytest.skip("pyserial not installed (python3 -m pip install pyserial)")
    baud = int(os.environ.get("ROTATOR_BAUD", "9600"))
    s = serial.Serial(_port(), baud, timeout=2)
    s.reset_input_buffer()
    time.sleep(0.2)
    # Warm up the USART DMA with a few round-trips before the first test.
    # Without this, single-byte commands (C, ?) fail on the first fresh
    # connection after the port has been closed and reopened.
    for _ in range(3):
        s.write(b"VE\r\n")
        s.readline()
    s.reset_input_buffer()
    yield s
    s.close()


@pytest.fixture(autouse=True)
def _drain(ser):
    """Drain any leftover bytes from the previous test before each test runs."""
    ser.reset_input_buffer()
    ser.timeout = 0.15  # long enough to catch delayed ST-LINK VCP buffered bytes
    while ser.read(256):
        pass
    ser.timeout = 2


# ── helpers ───────────────────────────────────────────────────────────────────

def send(s, cmd: str) -> None:
    """Send a command terminated with CR+LF (accepted by the firmware)."""
    s.write((cmd + "\r\n").encode())
    s.flush()


def recv_line(s) -> str:
    """Read until newline; strip trailing whitespace."""
    return s.readline().decode(errors="replace").strip()


def recv_pos(s) -> tuple[float, float]:
    """Read an AZ/EL response line and return (az, el).

    Retries once on timeout: the ST-LINK VCP occasionally buffers a response
    slightly beyond a single readline() window.
    """
    for attempt in range(2):
        line = recv_line(s)
        m = re.match(r"AZ([\d.]+)\s+EL([\d.]+)", line)
        if m:
            return float(m.group(1)), float(m.group(2))
        if attempt == 0 and line == "":
            continue  # retry once on timeout
    assert False, f"Expected AZ<n> EL<n>, got: {repr(line)}"


def poll_until(s, target_az: float, target_el: float,
               tolerance: float = 2.0, timeout: float = 60.0) -> tuple[float, float]:
    """Poll AZ until position is within tolerance of target, or timeout."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        send(s, "AZ")
        az, el = recv_pos(s)
        if abs(az - target_az) <= tolerance and abs(el - target_el) <= tolerance:
            return az, el
        time.sleep(0.5)
    pytest.fail(
        f"Motor did not reach AZ={target_az} EL={target_el} within {timeout} s "
        f"(last: AZ={az:.1f} EL={el:.1f})"
    )


# ── version ───────────────────────────────────────────────────────────────────

@pytest.mark.live
def test_version(ser):
    send(ser, "VE")
    resp = recv_line(ser)
    assert resp.startswith("Polar Pilot")


# ── keepalive ─────────────────────────────────────────────────────────────────

@pytest.mark.live
@pytest.mark.xfail(strict=False,
                   reason="bare-\\r response is 1 byte; ST-LINK VCP sometimes "
                          "delays single-byte TX beyond the read timeout")
def test_keepalive(ser):
    """? should respond with bare \\r."""
    send(ser, "?")
    resp = ser.read(1)
    assert resp == b"\r"


# ── get position ─────────────────────────────────────────────────────────────

@pytest.mark.live
def test_az_query_returns_position(ser):
    """AZ with no argument queries current position."""
    send(ser, "AZ")
    az, el = recv_pos(ser)
    assert 0.0 <= az <= 360.0
    assert 0.0 <= el <= 180.0


@pytest.mark.live
def test_c_query_returns_position(ser):
    """C (GS-232) also queries current position."""
    send(ser, "C")
    az, el = recv_pos(ser)
    assert 0.0 <= az <= 360.0
    assert 0.0 <= el <= 180.0


@pytest.mark.live
@pytest.mark.xfail(reason="rapid back-to-back AZ then C: TX DMA collision drops C response")
def test_az_and_c_agree(ser):
    """AZ and C return the same position."""
    send(ser, "AZ")
    az1, el1 = recv_pos(ser)
    send(ser, "C")
    az2, el2 = recv_pos(ser)
    assert abs(az1 - az2) < 1.0
    assert abs(el1 - el2) < 1.0


# ── stop ─────────────────────────────────────────────────────────────────────

@pytest.mark.live
def test_sa_sends_no_response(ser):
    """SA (stop AZ) produces no response line."""
    send(ser, "SA")
    ser.timeout = 0.3
    resp = ser.readline()
    assert resp == b"", f"Expected no response to SA, got {repr(resp)}"


@pytest.mark.live
def test_se_sends_no_response(ser):
    send(ser, "SE")
    ser.timeout = 0.3
    resp = ser.readline()
    assert resp == b""


@pytest.mark.live
def test_sa_se_combined(ser):
    send(ser, "SA SE")
    ser.timeout = 0.3
    resp = ser.readline()
    assert resp == b""


# ── set position ─────────────────────────────────────────────────────────────

@pytest.mark.live
@pytest.mark.slow
def test_az_el_set_both_axes(ser):
    """AZ<az> EL<el> moves both axes; position must converge."""
    send(ser, "AZ90.0 EL30.0")
    poll_until(ser, 90.0, 30.0)


@pytest.mark.live
@pytest.mark.slow
def test_w_command_sets_both_axes(ser):
    """GS-232B W<az> <el> moves both axes."""
    send(ser, "W 45.0 15.0")
    poll_until(ser, 45.0, 15.0)


@pytest.mark.live
@pytest.mark.slow
def test_a_command_sets_az_only(ser):
    """GS-232 A<NNN> moves azimuth, leaves elevation unchanged."""
    # First establish a known starting position
    send(ser, "AZ0.0 EL0.0")
    poll_until(ser, 0.0, 0.0)

    send(ser, "A120")
    poll_until(ser, 120.0, 0.0)


@pytest.mark.live
@pytest.mark.slow
def test_e_command_sets_el_only(ser):
    """GS-232 E<NNN> moves elevation, leaves azimuth unchanged."""
    send(ser, "AZ0.0 EL0.0")
    poll_until(ser, 0.0, 0.0)

    send(ser, "E45")
    poll_until(ser, 0.0, 45.0)


@pytest.mark.live
@pytest.mark.slow
def test_stop_halts_movement(ser):
    send(ser, "AZ180.0 EL45.0")
    time.sleep(1.5)  # let it start moving

    send(ser, "AZ")
    az_moving, _ = recv_pos(ser)

    send(ser, "SA SE")
    time.sleep(0.5)

    send(ser, "AZ")
    az_stopped, _ = recv_pos(ser)

    assert abs(az_stopped - az_moving) < 5.0, (
        f"Motor kept moving after stop: {az_moving:.1f} -> {az_stopped:.1f}"
    )


# ── limits ────────────────────────────────────────────────────────────────────

@pytest.mark.live
def test_lm_query_returns_four_values(ser):
    send(ser, "LM")
    line = recv_line(ser)
    assert line.startswith("LM ")
    parts = line[3:].split()
    assert len(parts) == 4, f"Expected 4 limit values, got: {repr(line)}"
    for p in parts:
        float(p)  # must be numeric


@pytest.mark.live
def test_lm_set_and_read_back(ser):
    send(ser, "LM 5.0 355.0 2.0 88.0")
    time.sleep(0.05)  # no response -- small delay before querying
    send(ser, "LM")
    line = recv_line(ser)
    parts = [float(x) for x in line[3:].split()]
    assert parts[0] == pytest.approx(5.0, abs=0.2)
    assert parts[1] == pytest.approx(355.0, abs=0.2)
    assert parts[2] == pytest.approx(2.0, abs=0.2)
    assert parts[3] == pytest.approx(88.0, abs=0.2)


@pytest.mark.live
def test_lm_restore_defaults(ser):
    """Restore defaults after limit tests so other tests are not affected."""
    send(ser, "LM 0.0 360.0 0.0 180.0")


# ── protocol robustness ───────────────────────────────────────────────────────

@pytest.mark.live
def test_lf_only_line_ending(ser):
    """Firmware also accepts bare LF (no CR)."""
    ser.write(b"VE\n")
    ser.flush()
    resp = recv_line(ser)
    assert resp.startswith("Polar Pilot")


@pytest.mark.live
def test_cr_only_line_ending(ser):
    """Firmware also accepts bare CR."""
    ser.write(b"VE\r")
    ser.flush()
    resp = recv_line(ser)
    assert resp.startswith("Polar Pilot")


@pytest.mark.live
def test_multiple_sequential_queries(ser):
    """Ten back-to-back AZ queries all return valid positions."""
    for _ in range(10):
        send(ser, "AZ")
        az, el = recv_pos(ser)
        assert 0.0 <= az <= 360.0
        assert 0.0 <= el <= 180.0


@pytest.mark.live
@pytest.mark.slow
def test_rh_rehomes(ser):
    """RH triggers the full endstop homing sequence."""
    send(ser, "RH")
    # During homing the phase is Homing; wait until position settles at 0/0.
    poll_until(ser, 0.0, 0.0, timeout=120.0)


@pytest.mark.live
@pytest.mark.slow
def test_rs_parks_to_zero(ser):
    """RS parks both axes to 0/0."""
    send(ser, "RS")
    poll_until(ser, 0.0, 0.0)


@pytest.mark.live
def test_unknown_command_produces_no_response(ser):
    """Unknown commands are silently ignored (firmware only warns via RTT)."""
    send(ser, "BOGUS")
    ser.timeout = 0.3
    resp = ser.readline()
    assert resp == b""
    # Subsequent valid command still works
    ser.timeout = 2
    send(ser, "VE")
    resp = recv_line(ser)
    assert resp.startswith("Polar Pilot")
