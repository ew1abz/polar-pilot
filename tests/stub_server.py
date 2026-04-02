"""
Faithful in-process stub of the Polar Pilot rotctld TCP server.

Mirrors the firmware's dispatch table so protocol tests can run without
hardware.  State is plain Python — mutate it directly in test fixtures.
"""

import socket
import threading


class RotatorState:
    def __init__(self):
        self.az: float = 0.0
        self.el: float = 0.0
        self.az_min: float = 0.0
        self.az_max: float = 360.0
        self.el_min: float = 0.0
        self.el_max: float = 180.0
        self.fault: str | None = None  # non-None → all mutating commands return RPRT -9


VERSION = "0.2.0"


def _dispatch(line: str, state: RotatorState) -> str | None:
    """Return response string, or None to close the connection."""

    def fault_guard():
        if state.fault:
            return f"FAULT: {state.fault}\nRPRT -9\n"
        return None

    # ── get_pos ─────────────────────────────────────────────────────────────
    if line in ("p", r"\get_pos"):
        return f"{state.az:.1f}\n{state.el:.1f}\n"

    # ── set_pos ─────────────────────────────────────────────────────────────
    if line.startswith("P ") or line.startswith(r"\set_pos "):
        if (r := fault_guard()):
            return r
        args = line[2:] if line.startswith("P ") else line[9:]
        parts = args.split()
        if len(parts) < 2:
            return "RPRT -1\n"
        try:
            az, el = float(parts[0]), float(parts[1])
        except ValueError:
            return "RPRT -1\n"
        state.az = max(state.az_min, min(state.az_max, az))
        state.el = max(state.el_min, min(state.el_max, el))
        return "RPRT 0\n"

    # ── stop ────────────────────────────────────────────────────────────────
    if line in ("S", r"\stop"):
        if (r := fault_guard()):
            return r
        return "RPRT 0\n"

    # ── move ────────────────────────────────────────────────────────────────
    if line.startswith("M ") or line.startswith(r"\move "):
        if (r := fault_guard()):
            return r
        args = line[2:] if line.startswith("M ") else line[6:]
        parts = args.split()
        if not parts:
            return "RPRT -1\n"
        try:
            direction = int(parts[0])
        except ValueError:
            return "RPRT -1\n"
        if direction == 2:    # Up
            state.el = state.el_max
        elif direction == 4:  # Down
            state.el = state.el_min
        elif direction == 8:  # Left / CCW
            state.az = state.az_min
        elif direction == 16: # Right / CW
            state.az = state.az_max
        else:
            return "RPRT -1\n"
        return "RPRT 0\n"

    # ── get_limits ──────────────────────────────────────────────────────────
    if line in ("l", r"\get_limits"):
        return (
            f"{state.az_min:.1f}\n{state.az_max:.1f}\n"
            f"{state.el_min:.1f}\n{state.el_max:.1f}\nRPRT 0\n"
        )

    # ── set_limits ──────────────────────────────────────────────────────────
    if line.startswith("L ") or line.startswith(r"\set_limits "):
        args = line[2:] if line.startswith("L ") else line[12:]
        parts = args.split()
        if len(parts) < 4:
            return "RPRT -1\n"
        try:
            az_min, az_max, el_min, el_max = (float(p) for p in parts[:4])
        except ValueError:
            return "RPRT -1\n"
        state.az_min, state.az_max = az_min, az_max
        state.el_min, state.el_max = el_min, el_max
        return "RPRT 0\n"

    # ── reset ────────────────────────────────────────────────────────────────
    if line in ("R", r"\reset") or line.startswith("R ") or line.startswith(r"\reset "):
        if (r := fault_guard()):
            return r
        state.az = 0.0
        state.el = 0.0
        return "RPRT 0\n"

    # ── send_cmd (stub — not implemented) ────────────────────────────────────
    if line.startswith("w ") or line.startswith(r"\send_cmd "):
        return "RPRT -1\n"

    # ── get_info ─────────────────────────────────────────────────────────────
    if line in ("_", r"\get_info"):
        return (
            f"Model name:\tPolar Pilot\n"
            f"Model ID:\t2\n"
            f"Mfg name:\tCustom\n"
            f"SW version:\t{VERSION}\n"
            f"Status:\t\tAlpha\n"
            f"Max AZ:\t\t450\n"
            f"Max EL:\t\t180\n"
            f"RPRT 0\n"
        )

    # ── dump_state ───────────────────────────────────────────────────────────
    if line == r"\dump_state":
        return (
            f"0\nrot_model=0\n"
            f"min_az={state.az_min:.1f}\nmax_az={state.az_max:.1f}\n"
            f"min_el={state.el_min:.1f}\nmax_el={state.el_max:.1f}\n"
            f"0\n0\nRPRT 0\n"
        )

    # ── dump_caps ────────────────────────────────────────────────────────────
    if line in ("1", r"\dump_caps"):
        return (
            f"Caps dump for model: 2\n"
            f"Model name:\tPolar Pilot\n"
            f"Mfg name:\tCustom\n"
            f"Backend version:\t{VERSION}\n"
            f"Backend status:\tAlpha\n"
            f"Rotator type:\tAz-El\n"
            f"Can set position:\tY\n"
            f"Can get position:\tY\n"
            f"Can stop:\tY\n"
            f"Can reset:\tY\n"
            f"Can move:\tY\n"
            f"Min Azimuth:\t{state.az_min:.2f}\n"
            f"Max Azimuth:\t{state.az_max:.2f}\n"
            f"Min Elevation:\t{state.el_min:.2f}\n"
            f"Max Elevation:\t{state.el_max:.2f}\n"
            f"RPRT 0\n"
        )

    # ── quit ─────────────────────────────────────────────────────────────────
    if line in ("q", r"\quit"):
        return None  # signal to close connection

    # ── unknown command ───────────────────────────────────────────────────────
    return "RPRT -8\n"


def _handle_client(conn: socket.socket, state: RotatorState) -> None:
    buf = b""
    with conn:
        while True:
            try:
                chunk = conn.recv(256)
            except OSError:
                break
            if not chunk:
                break
            buf += chunk
            while b"\n" in buf:
                raw, buf = buf.split(b"\n", 1)
                line = raw.decode(errors="replace").strip()
                resp = _dispatch(line, state)
                if resp is None:
                    return
                try:
                    conn.sendall(resp.encode())
                except OSError:
                    return


class StubServer:
    """Session-scoped TCP stub.  Reset ``state`` between tests."""

    def __init__(self, host: str = "127.0.0.1", port: int = 0):
        self._sock = socket.socket()
        self._sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._sock.bind((host, port))
        self.port: int = self._sock.getsockname()[1]
        self.state = RotatorState()

    def start(self) -> None:
        self._sock.listen(8)
        t = threading.Thread(target=self._accept_loop, daemon=True)
        t.start()

    def _accept_loop(self) -> None:
        while True:
            try:
                conn, _ = self._sock.accept()
            except OSError:
                break
            # Each client gets its own thread but shares state.
            t = threading.Thread(
                target=_handle_client, args=(conn, self.state), daemon=True
            )
            t.start()
