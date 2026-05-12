# Implementation Plan: Phase 4 - Rust Integration

`docs/plan/Tranceparent Python Backend.md` の Phase 4 「Rust 接続」を達成するための具体的な実装計画です。
Rust (Bevy) 側から Python バックエンドへ gRPC 経由で接続し、ライブデータを取得できるようにします。

## 1. 目的
- Rust 側で `tonic` を用いた gRPC クライアントを実装する。
- Bevy のメインループをブロックせずにバックグラウンドでデータを取得する。
- 既存の内部シミュレーションとバックエンド接続をトグル（切り替え）可能にする。

## 2. 実装内容

### 2.1 gRPC 通信基盤 (tonic + prost)
- `Cargo.toml` に `tonic`, `prost`, `tokio` を追加する。
- `build.rs` を作成し、`python/proto/engine.proto` から Rust コードを自動生成する。
- 生成されたコードは `mod` 経由で `src` から利用可能にする。

### 2.2 非同期データ取得タスク
Bevy の更新ループを止めないため、専用の async task を立ち上げます。

- **`BackendClient` (Resource)**: 
  - `tokio::sync::mpsc` の受信端を持つ。
  - バックグラウンドタスクが取得した最新の `TradingState` を受信する。
- **Background Task**:
  - `tokio` ランタイム上で動作するループ。
  - 設定されたインターバルで `GetState` を実行。
  - 取得したデータを channel で送信。
  - 接続失敗時のリトライロジック（指数バックオフ等）を実装。

### 2.3 トグル可能なシミュレーション (Backend Enabled)
既存のシミュレーションを残しつつ、バックエンド利用を選択可能にします。

- **`TradingSettings` (Resource)**:
  - `backend_enabled: bool`
  - `backend_url: String`
  - `token: String`
  - `poll_interval_ms: u64`
- **`price_simulation_system` の修正**:
  - `TradingSettings.backend_enabled` が `false` の場合のみ実行。
- **`backend_update_system` の新設**:
  - `backend_enabled` が `true` の場合に実行。
  - channel からデータを取り出し、`TradingData` を更新する。

### 2.4 設定管理
Rust 側の設定ファイル（または環境変数）で接続情報を管理できるようにします。

- 接続先 URL (Default: `http://127.0.0.1:50051`)
- 認証用 Token
- ポーリング間隔
- 失敗時のフォールバック挙動（エラー表示のみか、一時的にシミュレーションに戻るか等）

## 3. 実装ステップ

### Step 1: 依存関係と `build.rs` の設定
- `tonic-build` を導入し、proto からのコード生成を確認する。

### Step 2: 設定リソースとトグルロジックの導入
- `TradingSettings` を定義し、既存のシミュレーションシステムに `run_if` 等で条件を付ける。

### Step 3: gRPC クライアントと非同期タスクの実装
- `tokio` ランタイムの初期化（Bevy 連携）。
- `GetState` を定期実行するループの実装。

### Step 4: UI への反映
- バックエンドから受信した `json_data` をパースし、`TradingData` の `price` と `history` を更新する。

## 4. テスト仕様 (Phase 4)

| カテゴリ | 検証項目 | 期待される結果 |
| :--- | :--- | :--- |
| **接続** | 正常接続 | バックエンド起動中、Rust 側がデータを取得し UI が更新されること |
| | 認証失敗 | 不正な Token の場合、Rust 側でエラーログが出力され、UI 更新が止まる（またはシミュレーションを維持）こと |
| **堅牢性** | バックエンド停止 | 通信エラー時に Bevy がクラッシュせず、再接続を試みること |
| | 非ブロック | gRPC 通信中も Rust 側の画面描画や操作がスムーズに行えること |
| **機能** | トグル切り替え | 設定変更により、シミュレーションとバックエンドデータが正しく切り替わること |

## 5. 次のフェーズへの橋渡し
Phase 4 で Rust 接続が安定した後、Phase 5 では大量の履歴データやリアルタイム性の高いストリーミングデータへの対応を行い、チャート描画のパフォーマンスを最適化します。
