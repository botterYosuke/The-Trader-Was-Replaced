---
name: tdd-workflow
description: Use this skill when writing new features, fixing bugs, or refactoring code in The-Trader-Was-Replaced. Enforces test-driven development for both Python (pytest) and Rust (cargo test) layers. Trigger whenever the user says "implement", "add feature", "fix bug", "refactor", "write tests", or is about to touch any file in python/engine/, src/, or python/tests/. 日本語トリガー: 「実装して」「バグ修正」「リファクタ」「テスト書いて」「src/ を変更」「レビュー指摘を修正」「コンパイルエラー修正」「テスト破損を直す」「cargo test が落ちた」など。
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

**Rust の compile-order 制約**: Rust では参照先のシンボル（関数・型・enum variant）が
存在しないとテスト自体がコンパイルできないため、Python のような厳密な「テスト先行 RED」が
取れない。Rust UI system（Bevy）や新 enum variant を足すときは「実装 + 既存テスト修正を 1 手」
→ その後にテスト追加、という順になる。**RED 先行の主戦場は Python 側**に置き、Rust は
コンパイルが通る最小単位ごとに `cargo check`（test 抜き）→ テスト追加後に `cargo check --tests`
の順で刻むと中間状態の破壊が最小になる。

**serde/struct フィールド追加の全リテラル破壊トラップ**: `#[derive]` 構造体や serde 構造体に
フィールドを 1 つ足すと、`..Default::default()` を使わず**全フィールドを明示**しているテスト
リテラルが軒並み `missing field` でコンパイルエラーになる。フィールド追加時は
`rg "StructName \{"` で全構築箇所を洗い出し、同じ 1 手で `field: None,` 追記 or
`..Default::default()` 化すること（本番側は `::default()` 経由で無傷でも、テスト fixture が
明示リテラルだと割れる）。

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

### WRONG: テスト関数内で `import engine.x.y` するとローカル変数 `engine` が上書きされる
```python
def test_something():
    engine = DataEngine(...)        # DataEngine インスタンス
    import engine.strategy_runtime.strategy_loader  # ← Python の仕様で engine が module に変わる!
    engine._last_replay_catalog_path = str(tmp_path / "cat")  # DataEngine ではなく module に set!
```

### CORRECT: テスト冒頭で文字列 patch のみ使い、インスタンス変数名を engine 以外にするか、lazy import を書かない
```python
def test_something(monkeypatch):
    de = DataEngine(...)            # 変数名を engine にしない
    monkeypatch.setattr(
        "engine.strategy_runtime.strategy_loader.load", mock_load
    )
    de._last_replay_catalog_path = str(tmp_path / "cat")  # 正しく de に set される
```

### WRONG: `monkeypatch.setattr(engine.__class__, "prop", ...)` でクラスを汚染
```python
monkeypatch.setattr(engine.__class__, "jquants_loader_base_dir", property(lambda self: str(tmp_path)))
# DataEngine クラス自体が変更され、同セッション内の全インスタンスに副作用が出る
```

### CORRECT: コンストラクタで依存を渡す（クラスに触らない）
```python
_jq_loader = mock.MagicMock()
_jq_loader.base_dir = tmp_path / "jq"
de = DataEngine(jquants_loader=_jq_loader)  # property が自然に base_dir を返す
```

### WRONG: SUT が `.__name__` を参照する箇所を素の MagicMock で mock する
```python
strategy_cls = mock.MagicMock()
# server_grpc.py 内で strategy_cls.__name__ を f-string に使うと AttributeError
```

### CORRECT: `__name__` を明示的に設定する
```python
strategy_cls = mock.MagicMock(__name__="MockStrategy")
```

### WRONG: monkeypatch したい関数がテスト対象ファイル内で lazy import されている
```python
# server_grpc.py の StartEngine 内部:
def StartEngine(self, ...):
    from engine.strategy_runtime.engine_runner import run as engine_run  # 関数スコープ import
    ...
# monkeypatch.setattr("engine.server_grpc.engine_run", mock_run) が効かない
```

### CORRECT: module レベルで import し、monkeypatch ターゲットとして確立する
```python
# server_grpc.py のトップレベル:
from engine.strategy_runtime.engine_runner import run as engine_run
# これで "engine.server_grpc.engine_run" が monkeypatch で差し替えられる
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

> **Windows / uv の落とし穴**: editable build が再走った直後（proto regen で `python` パッケージが rebuild される等）に `uv run pytest ...` が `error: uv trampoline failed to canonicalize script path` で即死することがある（pytest の `.exe` shim が無効化されるため）。その場合は **`uv run python -m pytest ...`** に切り替えると通る（trampoline shim を経由しない）。`live/` 系テストは cwd=`python/` 前提（import が `from engine.live.X import ...`）なので `cd python; uv run python -m pytest tests/live/...` で回す。

---

## gRPC 契約変更時のルール

`python/proto/engine.proto` を変更したら必ず：

1. Proto を再コンパイル（Rust 側は `cargo build` で `build.rs` が自動実行）
2. Python 側も生成コードを更新（再生成すると `engine_pb2_grpc.py` の絶対 import が復活するので、`from . import engine_pb2 as engine__pb2` に**再パッチ**する）
3. **新しい `rpc` を追加したら、その trait を実装している mock も全部追従させる** — `tests/backend_integration.rs` の `MyDataEngine` (DataEngine trait の mock) は、proto に `rpc Shutdown` を足すと `E0046: missing shutdown` でコンパイル不能になる。proto の rpc 追加時は同 PR で mock に stub メソッド（`Ok(Response::new(XxxResponse{...}))`）を足すこと。`cargo test --no-run` で E0046 を早期検知できる。**server-streaming rpc (`returns (stream X)`)** を足した場合は mock に `type XxxStream = tokio_stream::Empty<Result<X, Status>>;` (assoc type) と空ストリーム返しの handler を足す。`tokio-stream` は `[dev-dependencies]` でよい（本番 `cargo run` には不要）。
4. 両サイドのテストが通ることを確認してから merge

```bash
# Python proto 再生成（生成物は python/engine/proto/ に置く。--python_out=. は誤り）
cd python
uv run python -m grpc_tools.protoc -I proto --python_out=engine/proto --grpc_python_out=engine/proto proto/engine.proto
# 再パッチ: engine_pb2_grpc.py の `import engine_pb2 as engine__pb2` を
#          `from . import engine_pb2 as engine__pb2` に直す（rg で確認）

# Rust は cargo build で自動
cargo build
```

### server-streaming handler の at-exit ハング罠（sync ThreadPool servicer）

`server_grpc.py` の servicer は `grpc.server(ThreadPoolExecutor)` の **同期** handler。新しく
`def Xxx(self, request, context): ... yield ...` 形式の server-streaming handler を足すとき、
handler が `queue.Queue.get()` など **ブロッキング待ち** をするなら、クライアントが
stream を cancel した瞬間に worker thread が `get()` で **永久停止** する（cancel は
`get()` を起こさない）。`server.stop(0)` でもこのスレッドは死なず、**インタプリタ終了時に
`ThreadPoolExecutor` の atexit join がそのスレッドを待ち続け、pytest プロセス全体が
exit でハングする**。

- **症状が紛らわしい**: テストは「N passed」と表示されるのに **プロンプトが返らない**
  （テスト失敗ではなく "プロセスが終わらない"）。`uv run pytest -m "not slow"` が
  普段 ~25s なのに 10 分以上終わらない → これを疑う。
- **検知**: 単体ファイルを `timeout 60 ... pytest` で回し、`EXIT=124` かつ "N passed" 既出なら
  at-exit ハング確定（macOS は `timeout` 不在なので `gtimeout` か python subprocess watchdog で）。
- **修正**: handler 内で subscription 取得直後に `context.add_callback(sub.close)` を登録する。
  RPC 終了 (cancel / deadline / teardown) で `sub.close()` が走り、sentinel を流して
  `get()` を起こす → handler が抜け、worker が解放される。`try/finally: sub.close()` も
  冪等なら併用可。

---

## プロジェクト固有の Rust テスト落とし穴

- **edition は 2024**。`rustfmt` を `cargo fmt` 経由でなく単体実行するときは `rustfmt --edition 2024 <file>` と明示すること。`--edition 2021` や素の `rustfmt` は let chains を 2015 と誤認して `async fn` パースエラーを撒く。**`cargo fmt` の全体実行はリポジトリ全ファイルを巻き込む**ので、対象ファイルだけ整形したいときは `rustfmt --edition 2024 src/foo.rs` で限定する。
- **`src/main.rs` の `#[cfg(test)] mod tests` は bin target**。`cargo test --lib` には現れない。main.rs に足したテストは `cargo test --bin backcast`（または全 target の `cargo test`）で確認する。`--lib` の件数が増えないのは正常。
- ライブラリ側（`src/backend_supervisor.rs` 等、`src/lib.rs` 経由）のユニットテストは `cargo test --lib <module>` で見える。

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
