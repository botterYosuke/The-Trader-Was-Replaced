# はじめに

> 文中の `[G1]` などは、その挙動を保証する E2E flow の ID。一覧は [`tests/e2e/FLOWS.md`](../../tests/e2e/FLOWS.md) を参照。

このページでは、前提環境の準備から GUI の起動、最初の Replay 実行までの最短手順を説明します。Python エンジン（nautilus_trader）は Rust バイナリに **PyO3 で同一プロセスに埋め込まれており**（in-proc）、別プロセスのバックエンドを起動する必要はありません（旧 gRPC バックエンドは #64 / #68 で撤去済み）。GUI 上での実行手順の一次情報は `docs/strategy-replay.md` です。

## 前提

| 項目 | 内容 |
|---|---|
| Python | 3.10 以上 |
| uv | Python 依存の取得・実行に使用（`uv sync`） |
| cargo | Rust GUI のビルド・起動に使用 |
| catalog | ParquetDataCatalog。既定では `{ARTIFACTS_PATH}/jquants-catalog` を参照する |

Python 依存は次のように取得します。

```bash
cd python
uv sync
```

## 1. GUI（Rust / Bevy）を起動

GUI の起動はラッパースクリプト 1 本で完結します。別プロセスのバックエンドを立ち上げる手順はありません。スクリプトが `BACKEND_TRANSPORT=inproc` 等の環境変数設定・`__pycache__` 削除・`backcast.exe` 起動を一括で行います。

```powershell
.\scripts\run_inproc.ps1
# artifacts を別ドライブに置く場合:
.\scripts\run_inproc.ps1 -ArtifactsPath S:\artifacts
```

ビルド前提（初回のみの `cargo build` / `PYO3_PYTHON` 設定）、Python DLL（`PYTHON_DLL_DIR` / `0xC0000135`）の扱い、正常起動ログの詳細は、ルートの [README.md §起動方法](../../README.md#起動方法) を一次情報として参照してください [P12]。

## 2. 接続を確認

GUI 画面右下のフッターに次が表示されれば、in-proc バックエンドへの接続成功です。

```
state: IDLE  backend: OK
```

`backend: DISABLED` が続く場合は `BACKEND_ENABLED=true` が渡っていません（`run_inproc.ps1` で起動すればスクリプトが設定します）。

> backend 接続状態（`backend: OK`）は [G1]、切断後の自己修復（再接続で復帰）は [G2]、`BACKEND_ENABLED=false` 時の `backend: DISABLED`（と replay clock 非反映）は [G3] で保証されます。flow 一覧は [`tests/e2e/FLOWS.md`](../../tests/e2e/FLOWS.md) を参照。

## 3. 最初の Replay 実行

1. メニューバー左の **File(&F)** から **Open (Ctrl+O)** で戦略の **サイドカー JSON（`<strategy>.json`）** を選択する（ファイルダイアログは `.json` のみを表示する。同じ場所の `<strategy>.py` が自動で読み込まれる） [I5]/[I9]
2. Strategy Editor ウィンドウが開く [I5]
3. フッター中央の **▶** ボタンをクリックして Run を開始 [A1]
4. フッターが `state: RUNNING` になり、ボタンが **||**（一時停止／再開）に切り替わる [A2]
5. 完了すると `state: IDLE` に戻り、Run Result パネルが `Completed` になり fills / pnl が表示される [A1]/[B1]
6. チャートエリアに最新バーのローソク足（赤／緑）が描画される [K1]

詳細な手順は [Replay 実行](replay.md) を参照してください。

## 画面の見取り図

![画面構成](assets/screen-layout.drawio.svg)

| エリア | 位置 | 役割 |
|---|---|---|
| メニューバー | 上 | File / Edit / Venue メニュー [I1]/[I2] |
| サイドバー | 左 | 銘柄リスト＋価格、パネルを開くボタン、Settings [J13]/[M1]/[M6] |
| フッター | 下 | 実行モードトグル、再生コントロール、速度、Venue 状態、backend 状態 [I4]/[A1]/[G1] |
| フローティングウィンドウ | 中央 | Strategy Editor / Chart / Buying Power / Positions / Orders / Run Result [M1]/[K1] |

各エリアの詳細は [画面構成](screen-layout.md) を参照してください。
