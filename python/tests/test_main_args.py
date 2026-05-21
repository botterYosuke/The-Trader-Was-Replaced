"""
Tests for __main__.py argument parsing and serve() wiring.
Verifies that --jquants-catalog-path and JQUANTS_CATALOG_PATH env var
reach serve() correctly without starting a real server.
"""

from unittest.mock import patch

import pytest

from engine.__main__ import parse_args


def test_jquants_catalog_path_cli_arg():
    args = parse_args(["--token", "t", "--jquants-catalog-path", "/tmp/catalog"])
    assert args.jquants_catalog_path == "/tmp/catalog"


def test_jquants_catalog_path_default_is_none_when_env_absent(monkeypatch):
    monkeypatch.delenv("JQUANTS_CATALOG_PATH", raising=False)
    args = parse_args(["--token", "t"])
    assert args.jquants_catalog_path is None


def test_jquants_catalog_path_default_from_env(monkeypatch):
    monkeypatch.setenv("JQUANTS_CATALOG_PATH", "/env/catalog")
    # parse_args reads os.environ.get at call time via default=, but argparse
    # evaluates default= at parser.add_argument() time.  We must re-import to
    # pick up the new env value, so we call parse_args after patching os.environ.get.
    with patch("os.environ.get", return_value="/env/catalog"):
        args = parse_args(["--token", "t"])
    assert args.jquants_catalog_path == "/env/catalog"


def test_jquants_catalog_path_passed_to_serve(monkeypatch):
    monkeypatch.delenv("JQUANTS_CATALOG_PATH", raising=False)

    captured = {}

    def fake_serve(*a, **kw):
        captured.update(kw)

    with patch("engine.__main__.serve", fake_serve):
        from engine.__main__ import main
        with patch(
            "sys.argv",
            ["engine", "--token", "tok", "--jquants-catalog-path", "/my/catalog"],
        ):
            main()

    assert captured.get("jquants_catalog_path") == "/my/catalog"


@pytest.mark.parametrize("venue", ["TACHIBANA", "KABU", "MOCK"])
def test_live_venue_accepts_supported_venues(venue):
    """--live-venue must accept every venue the factory/server support, incl. MOCK (#9.3)."""
    args = parse_args(["--token", "t", "--live-venue", venue])
    assert args.live_venue == venue


def test_live_venue_default_is_none():
    args = parse_args(["--token", "t"])
    assert args.live_venue is None


def test_live_venue_rejects_unknown():
    with pytest.raises(SystemExit):
        parse_args(["--token", "t", "--live-venue", "BOGUS"])


def test_live_venue_mock_passed_to_serve(monkeypatch):
    """MOCK reaches serve() via the same path as real venues (start path is wired)."""
    monkeypatch.delenv("JQUANTS_CATALOG_PATH", raising=False)

    captured = {}

    def fake_serve(*a, **kw):
        captured.update(kw)

    with patch("engine.__main__.serve", fake_serve):
        from engine.__main__ import main
        with patch("sys.argv", ["engine", "--token", "tok", "--live-venue", "MOCK"]):
            main()

    assert captured.get("live_venue") == "MOCK"


def test_jquants_catalog_path_none_when_omitted(monkeypatch):
    monkeypatch.delenv("JQUANTS_CATALOG_PATH", raising=False)

    captured = {}

    def fake_serve(*a, **kw):
        captured.update(kw)

    with patch("engine.__main__.serve", fake_serve):
        from engine.__main__ import main
        with patch("sys.argv", ["engine", "--token", "tok"]):
            main()

    assert captured.get("jquants_catalog_path") is None
