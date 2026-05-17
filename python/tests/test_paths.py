"""TDD Red: paths.py が存在しないため全件 fail することを確認する"""
import pytest
from engine.paths import catalog_path, artifacts_dir


def test_catalog_path_uses_artifacts_path_env(monkeypatch, tmp_path):
    monkeypatch.setenv("ARTIFACTS_PATH", str(tmp_path))
    monkeypatch.delenv("BACKEND_CATALOG_PATH", raising=False)
    result = catalog_path()
    assert result.name == "jquants-catalog"
    assert result.parent == tmp_path


def test_catalog_path_defaults_to_artifacts_subdir(monkeypatch):
    monkeypatch.delenv("ARTIFACTS_PATH", raising=False)
    monkeypatch.delenv("BACKEND_CATALOG_PATH", raising=False)
    result = catalog_path()
    assert result.name == "jquants-catalog"
    assert "artifacts" in str(result)


def test_catalog_path_ignores_backend_catalog_path(monkeypatch, tmp_path):
    monkeypatch.delenv("ARTIFACTS_PATH", raising=False)
    monkeypatch.setenv("BACKEND_CATALOG_PATH", "/legacy/path")
    result = catalog_path()
    assert not str(result).startswith("/legacy/path")
    assert result.name == "jquants-catalog"


def test_artifacts_dir_returns_path_object(monkeypatch, tmp_path):
    monkeypatch.setenv("ARTIFACTS_PATH", str(tmp_path))
    result = artifacts_dir()
    assert result == tmp_path
