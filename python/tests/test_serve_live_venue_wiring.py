"""C1.3 RED: serve() / __main__ への --live-venue 配線テスト.

- parse_args が --live-venue を受け取り args.live_venue になる
- main() が args.live_venue を serve() に kwarg で渡す
- serve(live_venue="TACHIBANA") が GrpcDataEngineServer に
  build_live_adapter_factory 由来の live_adapter_factory を渡す
"""

from unittest.mock import patch, MagicMock

import pytest

from engine.__main__ import main, parse_args


def test_live_venue_cli_arg_parses():
    args = parse_args(["--token", "t", "--live-venue", "TACHIBANA"])
    assert args.live_venue == "TACHIBANA"


def test_live_venue_default_is_none():
    args = parse_args(["--token", "t"])
    assert args.live_venue is None


def test_live_venue_passed_to_serve():
    captured = {}

    def fake_serve(*a, **kw):
        captured.update(kw)

    with patch("engine.__main__.serve", fake_serve):
        with patch(
            "sys.argv",
            ["engine", "--token", "tok", "--live-venue", "KABU"],
        ):
            main()

    assert captured.get("live_venue") == "KABU"


def test_serve_wires_live_adapter_factory_when_venue_given():
    """serve(live_venue="TACHIBANA") -> GrpcDataEngineServer に factory が渡る."""
    from engine import server_grpc

    captured = {}

    class FakeServer:
        def __init__(self, token, engine, **kwargs):
            captured.update(kwargs)

        def __getattr__(self, name):
            return MagicMock()

    fake_grpc_server = MagicMock()
    fake_grpc_server.start.side_effect = KeyboardInterrupt()

    with patch.object(server_grpc, "GrpcDataEngineServer", FakeServer), \
         patch.object(server_grpc.grpc, "server", return_value=fake_grpc_server), \
         patch.object(server_grpc, "threading"):
        try:
            server_grpc.serve(port=0, token="t", live_venue="TACHIBANA")
        except KeyboardInterrupt:
            pass

    factory = captured.get("live_adapter_factory")
    assert factory is not None, "live_adapter_factory should be wired"
    assert callable(factory)


def test_serve_no_factory_when_live_venue_none():
    from engine import server_grpc

    captured = {}

    class FakeServer:
        def __init__(self, token, engine, **kwargs):
            captured.update(kwargs)

        def __getattr__(self, name):
            return MagicMock()

    fake_grpc_server = MagicMock()
    fake_grpc_server.start.side_effect = KeyboardInterrupt()

    with patch.object(server_grpc, "GrpcDataEngineServer", FakeServer), \
         patch.object(server_grpc.grpc, "server", return_value=fake_grpc_server), \
         patch.object(server_grpc, "threading"):
        try:
            server_grpc.serve(port=0, token="t")
        except KeyboardInterrupt:
            pass

    assert captured.get("live_adapter_factory") is None
