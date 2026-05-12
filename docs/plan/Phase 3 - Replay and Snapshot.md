# Implementation Plan: Phase 3 - Replay and Snapshot

`docs/plan/Tranceparent Python Backend.md` の Phase 3 「replay / snapshot 対応」を達成するための具体的な実装計画です。
e-station 由来の設計思想を取り入れ、将来の本格的なデータローダー (J-Quants/Nautilus) への差し替えを考慮した拡張性の高い土台を構築します。

## 1. 目的
- 過去データに基づく決定論的なバックエンド挙動を確立する。
- **インターフェースの分離**: データ供給、状態進行、状態取得を明確に分ける。
- **移植性の確保**: 将来的に e-station のリプレイエンジンへ差し替える際、Rust 側の契約（gRPC）を変更せずに済むようにする。

## 2. 実装内容

### 2.1 ReplayProvider インターフェース
`python/engine/replay.py` で抽象ベースクラスを定義します。

- **`BaseReplayProvider`**: データの読み込みとイテレーションの抽象定義。
- **`SimpleCSVProvider` (Phase 3 実装)**: 最小構成の CSV (`timestamp,price`) を読み込む具体クラス。将来の `JQuantsLoader` 等への差し替えポイント。
- **Schema**: `instrument_id`, `granularity` 等のメタデータを保持可能にし、CSV の `price` は内部的に `close` として扱う。

### 2.2 リプレイ進行メカニズムの分離
`GetState` (gRPC poll) とリプレイの歩みを分離します。

- **Decoupling**: `GetState` は単に「現在の最新状態」を返す Read-Only な操作とする。
- **Progression**: エンジン内部に `advance()` メソッドを持ち、内部タイマーまたは外部からのステップ要求によって進行する。
- **Deterministic Stepping**: テスト環境では明示的に `advance()` を呼ぶことで、1 ステップずつの決定論的な検証を可能にする。

### 2.3 スナップショットの定義 (EngineSnapshot)
DTO (`TradingState`) ではなく、エンジンの実行コンテキストの保存・復元に焦点を当てます。

- **`EngineSnapshot`**: `TradingState` に加え、リプレイの現在インデックス、ソース情報（ファイル名/メタデータ）、現在の進行モードを含む。
- **Scope**: StepBackward (巻き戻し) や異常終了後の再開を支えるための内部状態復元を目的とする。

### 2.4 将来互換 CLI 設計
e-station との整合性を考慮した引数予約を行います。

- `--mode`: `static` | `replay`
- `--replay-path`: (Phase 3 用) 簡易 CSV パス
- **Reserved (将来用)**: `--instrument-id`, `--start`, `--end`, `--granularity`, `--data-dir`
  - これらは Phase 3 では使用しないが、`argparse` で定義のみ行い、将来の差し替えを容易にする。

## 3. 実装ステップ

### Step 1: `BaseReplayProvider` の定義と `SimpleCSVProvider` 実装
- 抽象クラスによる境界の定義。
- CSV 読み込みと、読了時の挙動（`is_exhausted` フラグの管理、最終値の維持）の実装。

### Step 2: `DataEngine` のリファクタリング
- `get_current_state()` から進行ロジックを排除。
- 内部的な `_advance_one_step()` の実装。

### Step 3: `EngineSnapshot` ロジックの導入
- リプレイ状態（インデックス等）を含めたシリアライズの実装。

## 4. テスト仕様 (Phase 3)

| カテゴリ | 検証項目 | 期待される結果 |
| :--- | :--- | :--- |
| **Replay** | 進行の独立性 | `GetState` を複数回呼んでも、`advance()` を呼ばない限り `price` が変わらないこと |
| | 境界挙動 | データ末尾到達時に `is_exhausted=True` となり、最後の価格が維持されること |
| **Snapshot** | 内部状態復元 | インデックスを含めた復元により、リプレイの続きから正確に再開できること |
| **CLI** | 予約引数 | 将来用引数を渡してもエラーにならず、リプレイモードが正しく起動すること |

## 5. 次のフェーズへの橋渡し
進行 (Advance) と 取得 (Get) を分離することで、Phase 4 以降で「バックエンドは一定周期で自律的にリプレイを進め、Rust 側は好きなタイミングで最新状態を読み取る」という、実運用に近いストリーミング的な挙動の土台が完成します。
