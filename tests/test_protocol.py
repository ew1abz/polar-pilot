"""
Protocol tests for the Polar Pilot rotctld TCP server.

Run against the in-process stub by default.  All tests are stateless at
the TCP level -- state is set up directly on stub_server.state before
sending commands, so there is no sleeping or sequencing on motor timing.
"""

import pytest


# ── helpers ──────────────────────────────────────────────────────────────────

def send(s, cmd: str) -> None:
    s.sendall((cmd + "\n").encode())


def recv(s, n: int = 256) -> str:
    return s.recv(n).decode()


# ── get_pos ───────────────────────────────────────────────────────────────────

class TestGetPos:
    def test_short_form(self, conn_stub):
        s, state = conn_stub
        state.az, state.el = 123.4, 45.6
        send(s, "p")
        lines = recv(s).splitlines()
        assert len(lines) == 2
        assert abs(float(lines[0]) - 123.4) < 0.1
        assert abs(float(lines[1]) - 45.6) < 0.1

    def test_long_form(self, conn_stub):
        s, state = conn_stub
        state.az, state.el = 0.0, 0.0
        send(s, r"\get_pos")
        assert len(recv(s).splitlines()) == 2

    def test_initial_position_is_zero(self, conn_stub):
        s, _ = conn_stub
        send(s, "p")
        assert recv(s) == "0.0\n0.0\n"


# ── set_pos ───────────────────────────────────────────────────────────────────

class TestSetPos:
    def test_short_form(self, conn_stub):
        s, _ = conn_stub
        send(s, "P 180.0 45.0")
        assert recv(s) == "RPRT 0\n"

    def test_long_form(self, conn_stub):
        s, _ = conn_stub
        send(s, r"\set_pos 90.0 30.0")
        assert recv(s) == "RPRT 0\n"

    @pytest.mark.parametrize("az,el", [
        (0.0, 0.0),
        (360.0, 180.0),
        (180.5, 45.5),
    ])
    def test_valid_positions(self, conn_stub, az, el):
        s, _ = conn_stub
        send(s, f"P {az} {el}")
        assert recv(s) == "RPRT 0\n"

    def test_missing_arg_returns_error(self, conn_stub):
        s, _ = conn_stub
        send(s, "P 90.0")
        r = recv(s)
        assert r.startswith("RPRT -")

    def test_non_numeric_returns_error(self, conn_stub):
        s, _ = conn_stub
        send(s, "P abc def")
        r = recv(s)
        assert r.startswith("RPRT -")


# ── stop ─────────────────────────────────────────────────────────────────────

class TestStop:
    def test_short_form(self, conn_stub):
        s, _ = conn_stub
        send(s, "S")
        assert recv(s) == "RPRT 0\n"

    def test_long_form(self, conn_stub):
        s, _ = conn_stub
        send(s, r"\stop")
        assert recv(s) == "RPRT 0\n"


# ── move ─────────────────────────────────────────────────────────────────────

class TestMove:
    @pytest.mark.parametrize("direction", [2, 4, 8, 16])
    def test_valid_directions(self, conn_stub, direction):
        s, _ = conn_stub
        send(s, f"M {direction} 50")
        assert recv(s) == "RPRT 0\n"

    def test_long_form(self, conn_stub):
        s, _ = conn_stub
        send(s, r"\move 2 100")
        assert recv(s) == "RPRT 0\n"

    def test_invalid_direction_returns_error(self, conn_stub):
        s, _ = conn_stub
        send(s, "M 99 50")
        r = recv(s)
        assert r.startswith("RPRT -")

    def test_non_numeric_direction(self, conn_stub):
        s, _ = conn_stub
        send(s, "M up 50")
        r = recv(s)
        assert r.startswith("RPRT -")


# ── get_limits ────────────────────────────────────────────────────────────────

class TestGetLimits:
    def test_returns_four_values_plus_rprt(self, conn_stub):
        s, state = conn_stub
        state.az_min, state.az_max = 0.0, 360.0
        state.el_min, state.el_max = 0.0, 180.0
        send(s, "l")
        lines = recv(s).splitlines()
        assert len(lines) == 5
        assert lines[4] == "RPRT 0"
        assert [float(l) for l in lines[:4]] == [0.0, 360.0, 0.0, 180.0]

    def test_long_form(self, conn_stub):
        s, _ = conn_stub
        send(s, r"\get_limits")
        lines = recv(s).splitlines()
        assert lines[-1] == "RPRT 0"


# ── set_limits ────────────────────────────────────────────────────────────────

class TestSetLimits:
    def test_short_form(self, conn_stub):
        s, _ = conn_stub
        send(s, "L 10.0 350.0 5.0 85.0")
        assert recv(s) == "RPRT 0\n"

    def test_limits_are_stored(self, conn_stub):
        s, _ = conn_stub
        send(s, "L 10.0 350.0 5.0 85.0")
        recv(s)
        send(s, "l")
        lines = recv(s).splitlines()
        assert float(lines[0]) == pytest.approx(10.0)
        assert float(lines[1]) == pytest.approx(350.0)
        assert float(lines[2]) == pytest.approx(5.0)
        assert float(lines[3]) == pytest.approx(85.0)

    def test_missing_args_returns_error(self, conn_stub):
        s, _ = conn_stub
        send(s, "L 0.0 360.0")
        r = recv(s)
        assert r.startswith("RPRT -")

    def test_long_form(self, conn_stub):
        s, _ = conn_stub
        send(s, r"\set_limits 0.0 360.0 0.0 90.0")
        assert recv(s) == "RPRT 0\n"


# ── reset ─────────────────────────────────────────────────────────────────────

class TestReset:
    @pytest.mark.parametrize("rtype", ["", " 0", " 1", " 2"])
    def test_reset_types(self, conn_stub, rtype):
        s, _ = conn_stub
        send(s, f"R{rtype}")
        assert recv(s) == "RPRT 0\n"

    def test_long_form(self, conn_stub):
        s, _ = conn_stub
        send(s, r"\reset 1")
        assert recv(s) == "RPRT 0\n"

    def test_parks_to_zero(self, conn_stub):
        s, state = conn_stub
        state.az, state.el = 90.0, 45.0
        send(s, "R 1")
        recv(s)
        send(s, "p")
        lines = recv(s).splitlines()
        assert float(lines[0]) == pytest.approx(0.0)
        assert float(lines[1]) == pytest.approx(0.0)

    def test_r2_rehome_returns_rprt0(self, conn_stub):
        s, _ = conn_stub
        send(s, "R 2")
        assert recv(s) == "RPRT 0\n"

    def test_r2_long_form(self, conn_stub):
        s, _ = conn_stub
        send(s, r"\reset 2")
        assert recv(s) == "RPRT 0\n"


# ── send_cmd ──────────────────────────────────────────────────────────────────

class TestSendCmd:
    def test_short_form_returns_error(self, conn_stub):
        s, _ = conn_stub
        send(s, "w anything")
        assert recv(s) == "RPRT -1\n"

    def test_long_form_returns_error(self, conn_stub):
        s, _ = conn_stub
        send(s, r"\send_cmd anything")
        assert recv(s) == "RPRT -1\n"


# ── get_info ─────────────────────────────────────────────────────────────────

class TestGetInfo:
    def test_short_form_ends_with_rprt0(self, conn_stub):
        s, _ = conn_stub
        send(s, "_")
        r = recv(s, 1024)
        assert r.endswith("RPRT 0\n")
        assert len(r.splitlines()) > 1

    def test_long_form(self, conn_stub):
        s, _ = conn_stub
        send(s, r"\get_info")
        r = recv(s, 1024)
        assert r.endswith("RPRT 0\n")

    def test_contains_model_name(self, conn_stub):
        s, _ = conn_stub
        send(s, "_")
        r = recv(s, 1024)
        assert "Polar Pilot" in r


# ── dump_state ────────────────────────────────────────────────────────────────

class TestDumpState:
    def test_ends_with_rprt0(self, conn_stub):
        s, _ = conn_stub
        send(s, r"\dump_state")
        r = recv(s, 1024)
        assert r.endswith("RPRT 0\n")

    def test_contains_az_el_limits(self, conn_stub):
        s, state = conn_stub
        state.az_min, state.az_max = 5.0, 355.0
        send(s, r"\dump_state")
        r = recv(s, 1024)
        assert "5.0" in r
        assert "355.0" in r


# ── dump_caps ─────────────────────────────────────────────────────────────────

class TestDumpCaps:
    def test_short_form_ends_with_rprt0(self, conn_stub):
        s, _ = conn_stub
        send(s, "1")
        r = recv(s, 4096)
        assert r.endswith("RPRT 0\n")

    def test_long_form(self, conn_stub):
        s, _ = conn_stub
        send(s, r"\dump_caps")
        r = recv(s, 4096)
        assert r.endswith("RPRT 0\n")

    def test_contains_required_fields(self, conn_stub):
        s, _ = conn_stub
        send(s, "1")
        r = recv(s, 4096)
        for field in ("Model name", "Mfg name", "Can set position", "Can get position"):
            assert field in r


# ── quit ─────────────────────────────────────────────────────────────────────

class TestQuit:
    def test_short_form_closes_connection(self, conn_stub):
        s, _ = conn_stub
        send(s, "q")
        import time; time.sleep(0.05)
        s.settimeout(1.0)
        assert s.recv(64) == b""

    def test_long_form_closes_connection(self, conn_stub):
        s, _ = conn_stub
        send(s, r"\quit")
        import time; time.sleep(0.05)
        s.settimeout(1.0)
        assert s.recv(64) == b""


# ── error cases ───────────────────────────────────────────────────────────────

class TestErrorCases:
    def test_unknown_command_returns_rprt_minus8(self, conn_stub):
        s, _ = conn_stub
        send(s, "X")
        assert recv(s) == "RPRT -8\n"

    def test_unknown_long_command(self, conn_stub):
        s, _ = conn_stub
        send(s, r"\totally_unknown")
        assert recv(s) == "RPRT -8\n"

    def test_empty_line_does_not_crash(self, conn_stub):
        s, _ = conn_stub
        send(s, "")
        recv(s)          # consume RPRT -8 for the empty command
        send(s, "p")
        assert len(recv(s).splitlines()) == 2

    def test_whitespace_line_does_not_crash(self, conn_stub):
        s, _ = conn_stub
        send(s, "   ")
        recv(s)          # consume RPRT -8 for the whitespace command
        send(s, "p")
        assert len(recv(s).splitlines()) == 2


# ── protocol robustness ───────────────────────────────────────────────────────

class TestProtocolRobustness:
    def test_crlf_line_endings(self, conn_stub):
        s, _ = conn_stub
        s.sendall(b"p\r\n")
        r = recv(s)
        assert len(r.splitlines()) == 2

    def test_pipelined_commands(self, conn_stub):
        """Multiple commands in a single TCP write."""
        s, _ = conn_stub
        s.sendall(b"S\np\nS\n")
        import time; time.sleep(0.05)
        r = s.recv(1024).decode()
        # S → RPRT 0, p → az\nel, S → RPRT 0
        assert r.count("RPRT 0") == 2
        assert len(r.splitlines()) == 4

    def test_split_tcp_segments(self, conn_stub):
        """Command split across two recv() calls."""
        import time
        s, _ = conn_stub
        s.sendall(b"p")    # no newline yet
        time.sleep(0.01)
        s.sendall(b"\n")   # complete it
        r = recv(s)
        assert len(r.splitlines()) == 2

    def test_oversized_line_closes_connection(self, conn_stub):
        """Line longer than firmware's 128-byte buffer must not leave it hung."""
        import time
        s, _ = conn_stub
        s.sendall(b"w " + b"A" * 300 + b"\n")
        time.sleep(0.05)
        # stub closes or returns an error -- either is acceptable
        s.settimeout(1.0)
        r = s.recv(256).decode()
        assert "RPRT" in r or r == ""

    def test_multiple_sequential_commands(self, conn_stub):
        s, state = conn_stub
        state.az, state.el = 90.0, 45.0
        for _ in range(10):
            send(s, "p")
            lines = recv(s).splitlines()
            assert len(lines) == 2
            assert abs(float(lines[0]) - 90.0) < 0.1
