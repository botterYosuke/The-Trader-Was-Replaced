# Implementation Plan: Phase 2 - Data Model Migration

`docs/plan/Tranceparent Python Backend.md` の Phase 2 「データモデル移植」を達成するための具体的な実装計画です。
Pydantic を導入し、Python バックエンドと Rust (Bevy) UI の間のデータ契約を厳密に定義します。

## 1. 目的
- Python 側で明示的なスキーマ（Pydantic モデル）を定義し、データの整合性を保証する。
- データのシリアライズ/デシリアライズにおけるヒューマンエラーを防止する。
- Rust 側が期待するデータ構造（`price`, `history` 等）との不一致を早期に検出する。

## 2. 実装内容

### 2.1 Pydantic モデルの定義
`python/engine/models.py` を新設し、以下のモデルを定義します。

```python
from pydantic import BaseModel, Field, ConfigDict
from typing import List
import time

class _BoundaryModel(BaseModel):
    """
    プロジェクト全体の基本設定を持つ基底モデル。
    strict=True により、型変換（coercion）を禁止し、厳密な型チェックを行う。
    extra="forbid" により、未定義のフィールドを禁止する。
    """
    model_config = ConfigDict(strict=True, extra="forbid", frozen=True)

class TradingState(_BoundaryModel):
    """
    Rust UI が受信を期待するバックエンドの全体状態（DTO）。
    """
    price: float = Field(..., description="現在の市場価格")
    history: List[float] = Field(default_factory=list, description="過去の価格履歴")
    timestamp: float = Field(
        default_factory=time.time, 
        description="データ生成時の Unix タイムスタンプ (秒)"
    )
```

### 2.2 Core エンジンの修正
`python/engine/core.py` の `get_current_state` を、辞書ではなく `TradingState` モデルを返すように変更します。
- 現行の `timer` は内部的な状態管理（更新間隔等）に使用し、外部への DTO (`TradingState`) には含めません。
- `timestamp` は `TradingState` 生成時に `time.time()` 等で付与します。

### 2.3 gRPC サーバーの修正
`python/engine/server_grpc.py` で `GetState` を処理する際、モデルをシリアライズして返すようにします。
- Pydantic の `model_dump_json()` を使用します。
- パフォーマンス向上のための `orjson` 導入は、将来の拡張（大量データ時）まで延期します。

### 2.4 サンプルデータの移行
Phase 1 でハードコードされていたサンプルデータを、Pydantic モデル経由で生成するように変更します。

## 3. 実装ステップ

### Step 1: `models.py` の作成
- `TradingState` モデルの実装。
- 必要に応じてバリデーションルール（例：price > 0）を追加。

### Step 2: `core.py` のリファクタリング
- 内部状態の保持を `TradingState` に移行。
- `get_current_state()` が `TradingState` インスタンスを返すように変更。

### Step 3: `server_grpc.py` の更新
- `json.dumps(state)` を `state.model_dump_json()` 等に置き換え。
- レスポンスの生成プロセスにバリデーションを組み込む。

### Step 4: テストの追加
- スキーマバリデーションのテスト（不正な値が拒否されるか）。
- JSON シリアライズ結果が Rust 側の期待値と一致するかの検証。

## 4. テスト仕様 (Phase 2)

| カテゴリ | 検証項目 | 期待される結果 |
| :--- | :--- | :--- |
| **バリデーション** | 正当なデータ | 正しく `TradingState` インスタンスが生成される |
| | 必須項目の欠落 | `ValidationError` が発生する |
| | 型の不一致 | `price="120.5"` (文字列) を渡した場合に `ValidationError` が発生する (strict) |
| | 未定義フィールド | `timer` を渡した場合に `ValidationError` が発生する (extra="forbid") |
| **シリアライズ** | JSON 出力 | `price`, `history`, `timestamp` が含まれる正しい形式の JSON が生成される |
| | Timer の除外 | 出力 JSON に `timer` が含まれていないこと |
| **統合** | gRPC レスポンス | `GetState` から返る `json_data` が `timestamp` を含み、`timer` を含まないこと |

## 5. 次のフェーズへの橋渡し
Phase 2 でデータ契約（DTO）が固定された後、Phase 3 では実際の replay データや snapshot データをこのモデルに流し込めるようにします。

**Rust 側への影響:**
現行の Rust `TradingData` ([src/trading.rs](../../src/trading.rs)) は `timer` を持っていますが、これは Bevy 内部のシステム用です。Python から送られてくる JSON はあくまで「表示・分析用 DTO」として定義し、Rust 側でのデシリアライズ先として `TradingState` (DTO) を別途定義するか、既存の `TradingData` を DTO と内部状態に分離することを検討します。
