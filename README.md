# The Trader Was Replaced

Nautilus Trader ベースの戦略リプレイ・評価エンジン。

Bevy(Rust) フロントエンドに Python エンジン (nautilus_trader) を **PyO3 で同一プロセスに埋め込む**
単一バイナリ構成。旧来の gRPC バックエンド（別プロセス + TCP/protobuf）は #64 / #68 で撤去済み。

## 起動方法

### GUI アプリを起動（In-proc）

Python エンジンは Rust プロセスに直接埋め込まれているため、別途バックエンドを起動する必要はない。
ラッパースクリプト 1 本で完結する。

```powershell
.\scripts\run_inproc.ps1
# artifacts を別ドライブに置く場合:
.\scripts\run_inproc.ps1 -ArtifactsPath S:\artifacts
```

スクリプトは `__pycache__` 削除・環境変数設定（`BACKEND_TRANSPORT=inproc` 等）・GUI 起動を一括実行する。

#### ビルド前提（初回のみ）

```powershell
uv venv                              # .venv 作成
$env:PYO3_PYTHON = "$PWD\.venv\Scripts\python.exe"
cargo build
```

ビルド環境の詳細（pyo3 バージョン・対応 Python・ABI3 設定）は [In-proc ビルド詳細](#in-proc-ビルド詳細) を参照。

#### 起動後のログ（正常）

```
[inproc] Python worker thread starting
[inproc] DataEngine initialized
[inproc] RustEventSink registered on DataEngine
[inproc] InprocLiveServer initialized (live_venue_id=None)
```

> **Windows WinError 6714**: `__pycache__` が存在すると Python の `FileFinder` がディレクトリを
> 再スキャンし、TxF フィルタードライバ (Windows Defender 等) に引っかかる。
> `run_inproc.ps1` が起動前に `__pycache__` を自動削除する。
> `sys.dont_write_bytecode=True` が Rust 側で自動設定されるため削除後は再作成されない。

---

### ヘッドレスリプレイ（GUI なし）

Python のみで戦略バックテストを実行する。Bevy GUI は不要。

→ **[docs/strategy-replay.md](docs/strategy-replay.md)**

```powershell
.\scripts\run_replay.ps1 -Strategy examples\test_strategy_daily.py
```

---

## In-proc ビルド詳細

| 項目 | 内容 |
|---|---|
| pyo3 バージョン | 0.22（Python 3.13 まで正式サポート） |
| 動作確認済み Python | 3.13 / 3.14（ABI3 前方互換モード） |
| `PYO3_USE_ABI3_FORWARD_COMPATIBILITY` | `.cargo/config.toml` で自動設定済み |
| `PYO3_PYTHON`（ビルド時） | `.venv\Scripts\python.exe` を指定 |
| 注意 | pyo3 を 0.23+ にアップグレードすれば ABI3 フラグ不要（issue #64 フォロータスク②） |

---

## ドキュメント

| ドキュメント | 内容 |
|---|---|
| [docs/strategy-replay.md](docs/strategy-replay.md) | 戦略リプレイの起動手順・CLI オプション |
| [docs/plan/Phase 6.5 - Blacksheep Strategy Runtime.md](docs/plan/Phase%206.5%20-%20Blacksheep%20Strategy%20Runtime.md) | Strategy Runtime 実装仕様 |
| [python/README.md](python/README.md) | Python エンジンのセットアップ・テスト |
