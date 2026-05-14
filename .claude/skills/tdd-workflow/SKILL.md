---
name: tdd-workflow
description: Use this skill when writing new features, fixing bugs, or refactoring code in The-Trader-Was-Replaced. Enforces test-driven development for both Python (pytest) and Rust (cargo test) layers. Trigger whenever the user says "implement", "add feature", "fix bug", "refactor", "write tests", or is about to touch any file in python/engine/, src/, or python/tests/.
origin: ECC (customized for The-Trader-Was-Replaced)
---

# Test-Driven Development Workflow — The-Trader-Was-Replaced

このプロジェクトは **Rust (Bevy GUI)** + **Python (Nautilus Trader engine)** の 2 層構成。
テストの主戦場は Python (`uv run pytest`)。Rust 側は gRPC 統合テストのみ。

## Architecture Overview

```
Rust (Bevy GUI)  ←── gRPC :19876 ──→  Python Engine (Nautilus Trader)
src/                                    python/engine/
  main.rs  trading.rs  ui/               server_grpc.py  core.py
                                         jquants_loader.py  replay.py
                                         strategy_replay/  strategy_runtime/
tests/                                  python/tests/
  backend_integration.rs                 test_engine.py  test_grpc_*.py
  (Rust integration tests)               test_nautilus_*.py  test_reducer.py
```

---

## When to Activate

- Python engine の新機能・ロジック追加
- gRPC エンドポイントの追加・変更
- Nautilus アダプター / J-Quants ローダーの変更
- リプレイ・ストラテジー実行ロジックの変更
- バグ修正・リファクタリング全般
- Rust 側の gRPC クライアント変更

---

## Core Principles

### 1. Tests BEFORE Code（Red → Green → Refactor）
テストを先に書き、失敗させてから実装する。

### 2. Test Placement

| テスト種別 | 場所 | コマンド |
|---|---|---|
| Python ユニット/統合 | `python/tests/test_*.py` | `uv run pytest` |
| 重いテスト（実データ） | `@pytest.mark.slow` でマーク | `uv run pytest -m slow` |
| Rust gRPC 統合 | `tests/backend_integration.rs` | `cargo test` |
| E2E（Bevy GUI 込み） | `e2e-testing` スキルを使用 | `/e2e-testing` |

### 3. Coverage Goal
ロジック・変換・エラーパスは 80% 以上。Bevy レンダリング部分はユニットテスト対象外で可。

---

## TDD Workflow Steps

### Step 1: シナリオを言語化
```
対象: python/engine/reducer.py
シナリオ: 日足バーが存在しない銘柄コードを渡したとき、
          空リストを返し例外を投げない
```

### Step 2: テストを先に書く（failing test）

```python
# python/tests/test_reducer.py
def test_reducer_returns_empty_for_unknown_symbol(reducer):
    """存在しない銘柄は空リストを返す（例外なし）"""
    result = reducer.reduce("9999999", bars=[])
    assert result == []
```

### Step 3: テストを実行して失敗を確認
```bash
uv run pytest python/tests/test_reducer.py::test_reducer_returns_empty_for_unknown_symbol -v
# FAILED が出ることを確認
```

### Step 4: 最小限の実装
テストが green になる最小のコードを書く。

### Step 5: 再実行して green 確認
```bash
uv run pytest python/tests/test_reducer.py -v
```

### Step 6: リファクタリング
テストを green に保ちながらコード品質を向上。

### Step 7: 全テストが壊れていないか確認
```bash
uv run pytest -m "not slow"
```

---

## Testing Patterns

### Python ユニットテスト（基本形）
```python
# python/tests/test_reducer.py
import pytest
from engine.reducer import OHLCReducer

@pytest.fixture
def reducer():
    return OHLCReducer()

def test_reduce_daily_bars_sorted_by_date(reducer, sample_bars):
    """バーが時系列順に並んでいることを確認"""
    result = reducer.reduce("7203", sample_bars)
    dates = [b.timestamp for b in result]
    assert dates == sorted(dates)

def test_reduce_empty_input_returns_empty(reducer):
    result = reducer.reduce("7203", bars=[])
    assert result == []
```

### gRPC エンドポイントテスト
```python
# python/tests/test_grpc_control.py
import pytest
from engine.server_grpc import EngineServicer

@pytest.fixture
def servicer(tmp_path):
    return EngineServicer()

@pytest.mark.asyncio
async def test_start_engine_requires_strategy_file(servicer, grpc_context):
    """strategy_file が空のとき INVALID_ARGUMENT を返す"""
    from proto import engine_pb2
    req = engine_pb2.StartEngineRequest(strategy_file="")
    resp = await servicer.StartEngine(req, grpc_context)
    # grpc_context に set_code が呼ばれていることを確認
    assert grpc_context.code_was_set()

@pytest.mark.asyncio
async def test_start_engine_valid_request_returns_ok(servicer, grpc_context, tmp_strategy_file):
    from proto import engine_pb2
    req = engine_pb2.StartEngineRequest(strategy_file=str(tmp_strategy_file))
    resp = await servicer.StartEngine(req, grpc_context)
    assert resp is not None
```

### Nautilus アダプターテスト
```python
# python/tests/test_nautilus_adapter.py
import pytest
from unittest.mock import AsyncMock, patch
from engine.nautilus_adapter import NautilusAdapter

@pytest.fixture
def adapter():
    return NautilusAdapter()

def test_adapter_converts_bar_to_nautilus_format(adapter, sample_bar):
    """BarData → Nautilus Bar の変換が正しい"""
    result = adapter.convert_bar(sample_bar)
    assert result.open.as_double() == pytest.approx(sample_bar.open)
    assert result.close.as_double() == pytest.approx(sample_bar.close)

def test_adapter_raises_on_invalid_symbol(adapter):
    with pytest.raises(ValueError, match="invalid symbol"):
        adapter.convert_bar(None)
```

### J-Quants ローダーテスト（実 API は skip）
```python
# python/tests/test_jquants_loader.py
import pytest
from unittest.mock import patch, MagicMock
from engine.jquants_loader import JQuantsLoader

@pytest.fixture
def loader():
    return JQuantsLoader(token="test-token")

def test_loader_parses_response_to_bars(loader):
    """API レスポンスが BarData リストに変換される"""
    mock_response = [
        {"Date": "2024-01-04", "Open": 2400.0, "High": 2450.0,
         "Low": 2390.0, "Close": 2430.0, "Volume": 1234567}
    ]
    with patch.object(loader, "_fetch_raw", return_value=mock_response):
        bars = loader.load("7203", start="2024-01-04", end="2024-01-04")
    assert len(bars) == 1
    assert bars[0].close == 2430.0

@pytest.mark.slow
def test_loader_real_api_returns_data(loader):
    """実 API を叩く統合テスト — token が必要"""
    bars = loader.load("7203", start="2024-01-04", end="2024-01-10")
    assert len(bars) > 0
```

### Rust 統合テスト（gRPC mock）
```rust
// tests/backend_integration.rs
use serial_test::serial;

#[tokio::test]
#[serial]
async fn grpc_client_connects_to_mock_server() {
    // Arrange: モック gRPC サーバーを起動
    let addr = "127.0.0.1:19900";
    let server = start_mock_server(addr).await;

    // Act: Rust gRPC クライアントで接続
    let mut client = EngineClient::connect(format!("http://{}", addr))
        .await
        .expect("should connect");

    // Assert: ヘルスチェックが通る
    let resp = client.health_check(Request::new(())).await;
    assert!(resp.is_ok());
    server.abort();
}
```

---

## ファイル配置

```
python/
├── tests/
│   ├── conftest.py              # pytest fixtures, markers
│   ├── data/                   # テスト用ストラテジーファイル
│   │   ├── test_strategy_7203_daily.py
│   │   └── test_strategy_7203_minute.py
│   ├── test_engine.py
│   ├── test_grpc_control.py
│   ├── test_grpc_replay.py
│   ├── test_reducer.py
│   ├── test_jquants_loader.py
│   ├── test_nautilus_adapter.py
│   └── test_data_engine_integration.py
└── engine/                     # 実装コード
    ├── server_grpc.py
    ├── core.py
    ├── reducer.py
    ├── jquants_loader.py
    └── nautilus_adapter.py

tests/
└── backend_integration.rs      # Rust 統合テスト
```

---

## よくある間違いと正しい書き方

### WRONG: 実装詳細をテスト
```python
assert engine._internal_state == "running"  # プライベートな状態
```

### CORRECT: 外部から観測できる振る舞いをテスト
```python
response = await servicer.GetStatus(req, ctx)
assert response.status == "running"
```

### WRONG: テスト間で状態を共有
```python
engine = EngineServicer()  # モジュールレベル — テスト間で汚染される
```

### CORRECT: fixture で毎回独立したインスタンス
```python
@pytest.fixture
def servicer():
    return EngineServicer()
```

### WRONG: 実 API に依存したユニットテスト
```python
def test_load_bars():
    loader = JQuantsLoader(token=os.environ["JQUANTS_TOKEN"])  # 環境依存
    bars = loader.load("7203")  # 実 API を叩く
    assert len(bars) > 0
```

### CORRECT: mock で外部依存を分離し、slow マーク
```python
def test_load_bars_parses_response(loader):
    with patch.object(loader, "_fetch_raw", return_value=FIXTURE_DATA):
        bars = loader.load("7203")
    assert len(bars) == len(FIXTURE_DATA)

@pytest.mark.slow
def test_load_bars_real_api(real_loader):  # 実 API はスローテスト
    bars = real_loader.load("7203")
    assert len(bars) > 0
```

---

## テスト実行コマンド早見表

```bash
# 高速テストのみ（CI 推奨）
uv run pytest -m "not slow" -v

# 全テスト（slow 含む）
uv run pytest -v

# 特定ファイル
uv run pytest python/tests/test_grpc_control.py -v

# 特定テスト名フィルター
uv run pytest -k "test_start_engine" -v

# 失敗時に即停止
uv run pytest -x

# 標準出力を表示
uv run pytest -s

# Rust 統合テスト
cargo test

# カバレッジ（pytest-cov が必要）
uv run pytest --cov=engine --cov-report=html
```

---

## gRPC 契約変更時のルール

`python/proto/engine.proto` を変更したら必ず：

1. Proto を再コンパイル（Rust 側は `cargo build` で `build.rs` が自動実行）
2. Python 側も生成コードを更新
3. 両サイドのテストが通ることを確認してから merge

```bash
# Python proto 再生成（プロジェクトルートで）
cd python
uv run python -m grpc_tools.protoc -I proto --python_out=. --grpc_python_out=. proto/engine.proto

# Rust は cargo build で自動
cargo build
```

---

## E2E テスト

Bevy GUI + Python engine を実際に起動する E2E 検証は **e2e-testing スキル**を使用。

```
/e2e-testing
```

---

## Success Metrics

- `uv run pytest -m "not slow"` が全 PASS
- ロジック部分のカバレッジ 80%+
- `cargo test` が全 PASS
- 追加したテストが独立して実行できる（順序依存なし）
- `@pytest.mark.slow` 以外に実 API・実 DB 依存がない

---

**Remember**: gRPC の型チェックは proto がカバーするが、ビジネスロジックのバグは防がない。テストがその安全網。
