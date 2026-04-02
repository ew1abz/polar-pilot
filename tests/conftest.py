import os
import socket

import pytest

from stub_server import RotatorState, StubServer


@pytest.fixture(scope="session")
def _stub_server():
    s = StubServer()
    s.start()
    return s


@pytest.fixture
def conn_stub(_stub_server):
    """Fresh TCP connection + clean state for each test."""
    _stub_server.state = RotatorState()
    s = socket.create_connection(("127.0.0.1", _stub_server.port), timeout=2)
    yield s, _stub_server.state
    s.close()


@pytest.fixture
def conn_live():
    """Connection to real hardware.  Skipped unless ROTATOR_HOST is set."""
    host = os.environ.get("ROTATOR_HOST")
    if not host:
        pytest.skip("ROTATOR_HOST not set")
    s = socket.create_connection((host, 4533), timeout=5)
    yield s
    s.close()
