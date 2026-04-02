"""
Live hardware tests for the Polar Pilot rotctld TCP server.

These tests require a running device.  Set ROTATOR_HOST to the device's
IP address before running:

    ROTATOR_HOST=192.168.1.42 pytest tests/test_live.py -v --timeout=60

All tests in this file are marked ``live`` and are excluded from CI via
``-m "not live"`` in pytest.ini.
"""

import os
import socket
import time

import pytest


def _host() -> str:
    h = os.environ.get("ROTATOR_HOST", "")
    if not h:
        pytest.skip("ROTATOR_HOST not set")
    return h


def _connect(timeout: float = 10.0) -> socket.socket:
    return socket.create_connection((_host(), 4533), timeout=timeout)


def send(s: socket.socket, cmd: str) -> None:
    s.sendall((cmd + "\n").encode())


def recv(s: socket.socket, n: int = 4096) -> str:
    return s.recv(n).decode()


# ── connectivity ──────────────────────────────────────────────────────────────

@pytest.mark.live
def test_dhcp_and_tcp_connect():
    """DHCP acquired and TCP socket accepted."""
    s = _connect()
    s.close()


@pytest.mark.live
def test_position_reported_as_float():
    with _connect() as s:
        send(s, "p")
        lines = recv(s).splitlines()
        assert len(lines) == 2
        float(lines[0])
        float(lines[1])


@pytest.mark.live
def test_get_info_ends_with_rprt0():
    with _connect() as s:
        send(s, "_")
        r = recv(s, 1024)
        assert r.endswith("RPRT 0\n")
        assert "Polar Pilot" in r


@pytest.mark.live
def test_dump_caps_ends_with_rprt0():
    with _connect() as s:
        send(s, "1")
        r = recv(s, 4096)
        assert r.endswith("RPRT 0\n")


# ── movement ──────────────────────────────────────────────────────────────────

@pytest.mark.live
@pytest.mark.slow
def test_set_pos_converges_to_target():
    """After P, repeated polling must converge within 60 s."""
    ctrl = _connect()
    mon = _connect()
    try:
        send(ctrl, "P 90.0 30.0")
        assert recv(ctrl) == "RPRT 0\n"
        deadline = time.time() + 60
        while time.time() < deadline:
            send(mon, "p")
            r = recv(mon)
            az, el = (float(x) for x in r.splitlines())
            if abs(az - 90.0) < 2.0 and abs(el - 30.0) < 2.0:
                return
            time.sleep(0.5)
        pytest.fail("Motor did not reach target within 60 s")
    finally:
        ctrl.close()
        mon.close()


@pytest.mark.live
@pytest.mark.slow
def test_stop_halts_movement():
    ctrl = _connect()
    mon = _connect()
    try:
        send(ctrl, "P 180.0 45.0")
        recv(ctrl)
        time.sleep(1.5)  # let it start moving

        send(mon, "p")
        az1 = float(recv(mon).splitlines()[0])

        send(ctrl, "S")
        assert recv(ctrl) == "RPRT 0\n"
        time.sleep(0.5)

        send(mon, "p")
        az2 = float(recv(mon).splitlines()[0])

        assert abs(az2 - az1) < 5.0, f"Motor kept moving after stop: {az1} → {az2}"
    finally:
        ctrl.close()
        mon.close()


@pytest.mark.live
@pytest.mark.slow
def test_reset_parks_to_zero():
    with _connect() as s:
        send(s, "R 1")
        assert recv(s) == "RPRT 0\n"
    # wait for motor to reach 0/0
    deadline = time.time() + 60
    while time.time() < deadline:
        with _connect() as s:
            send(s, "p")
            lines = recv(s).splitlines()
        az, el = float(lines[0]), float(lines[1])
        if abs(az) < 2.0 and abs(el) < 2.0:
            return
        time.sleep(0.5)
    pytest.fail("Motor did not park within 60 s")


# ── limits ────────────────────────────────────────────────────────────────────

@pytest.mark.live
def test_set_limits_accepted():
    with _connect() as s:
        send(s, "L 0.0 360.0 0.0 180.0")
        assert recv(s) == "RPRT 0\n"
        send(s, "l")
        lines = recv(s).splitlines()
        assert lines[-1] == "RPRT 0"


@pytest.mark.live
@pytest.mark.slow
def test_position_clamped_to_limits():
    """Command outside soft limits -- reported position must clamp, not exceed."""
    with _connect() as s:
        send(s, "L 85.0 95.0 25.0 35.0")
        recv(s)
        send(s, "P 180.0 80.0")
        recv(s)
    time.sleep(3.0)
    with _connect() as s:
        send(s, "p")
        lines = recv(s).splitlines()
    az, el = float(lines[0]), float(lines[1])
    assert az <= 95.0 + 2.0, f"AZ exceeded limit: {az}"
    assert el <= 35.0 + 2.0, f"EL exceeded limit: {el}"


# ── multi-client ──────────────────────────────────────────────────────────────

@pytest.mark.live
def test_two_concurrent_clients():
    c1 = _connect()
    c2 = _connect()
    try:
        send(c1, "p")
        send(c2, "p")
        r1 = recv(c1)
        r2 = recv(c2)
        assert len(r1.splitlines()) == 2
        assert len(r2.splitlines()) == 2
    finally:
        c1.close()
        c2.close()
