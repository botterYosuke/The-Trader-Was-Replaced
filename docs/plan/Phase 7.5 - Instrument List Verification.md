# Phase 7.5 — 実装確認 & 追加開発

> **位置づけ**: Phase 7 Closeout Checklist で「Phase 7 継続」と判定された Sidebar 銘柄一覧の実装が、Phase 8 の設計と矛盾していないことを確認・整合させる。新機能は追加しない。

---

## 0. 問題の背景

### Phase 7 の設計（`Phase 7 - Replay UI Integration.md` §3.3）

- Sidebar の銘柄一覧は `ListInstruments(source="replay")` RPC で取得
- **呼び出しタイミング**: `LoadStrategy` RPC 完了後、backend が `LOADED` に遷移したタイミングで 1 回
- **ソース**: `SCENARIO.end_date` のディレクトリ配下（例: `S:/j-quants/2024/04/15/`）の CSV ファイル名を列挙
- 返すのは「ストラテジーに登録した銘柄」ではなく、その日付カタログに存在する **全銘柄**（東証全銘柄相当）

### Phase 8 の設計（`Phase 8 - Live Venue and Market Data.md` §0.5.1 / 表 §0.5.1）

| モード | Sidebar 銘柄一覧ソース |
|---|---|
| Replay（未ログイン、初回起動） | Replay catalog（`source="replay"` フォールバック）|
| Replay（未ログイン、2 回目以降） | ローカル parquet（最後の Live スナップショット）|
| Live（Venue ログイン済み） | ローカル parquet（= ログイン時に上書き済み）|

- Phase 8 line 106: 「Phase 7 の『ローカル固定銘柄一覧』設計はこれにより廃止」と明言
- アプリ起動時のデフォルトモードは `LiveManual`（Venue 未ログイン可）。Sidebar は起動時から表示される

### 矛盾点（3 つ）

#### 矛盾 A: 銘柄一覧の呼び出しタイミング

- Phase 7 設計: `LoadStrategy` 完了後に 1 回呼ぶ → **戦略を開かないと Sidebar が空**
- 現在の実装 E2E ログ（2026-05-14 Sidebar → Bevy screen-space）: 「left-fixed Sidebar shows `1301.TSE` **on startup**」  
  → 戦略をロードする前から銘柄が表示されている。実装が Phase 7 設計書とずれている

#### 矛盾 B: Replay catalog の範囲

- Phase 7 設計書の表現は「`SCENARIO.end_date` のディレクトリ配下」→ ロードした戦略の `end_date` に依存
- しかし起動時から `1301.TSE` が出るということは、backend が固定パスか前回セッション残留データを返している可能性がある

#### 矛盾 C: Phase 8 との移行設計が未定義

- Phase 7 は `ListInstruments(source="replay")` を「replay catalog 専用 RPC」として実装
- Phase 8 では同 RPC の `source="live"` 追加と parquet キャッシュ優先が必要（Phase 7 doc §3.3 末尾に言及あり）
- Phase 7 の実装が Phase 8 の拡張に耐えられる構造になっているか未検証

---

## 1. Phase 7.5 の目標

1. **矛盾 A の真偽確認**: 現在の Rust 実装が「起動時」に `ListInstruments` を呼んでいるのか、「LoadStrategy 完了後」に呼んでいるのかをコードで確認する
2. **矛盾 B の真偽確認**: `source="replay"` 呼び出し時に backend が何を返しているかを確認する（固定パス？ 前回セッション？ catalog_path 引数？）
3. **矛盾 C の設計整合**: Phase 8 の `source="live"` 拡張に向けた RPC インターフェースが Phase 7 実装で阻害要因にならないことを確認する
4. **正しい仕様をドキュメントに明記**: 矛盾した記述を今回の確認結果で上書きする

### Phase 7.5 は実装しない

- parquet キャッシュの実装は Phase 8 スコープ
- `source="live"` の追加は Phase 8 スコープ
- 銘柄検索ボックス・仮想スクロールは Phase 8 スコープ
- Sidebar からの銘柄選択 → Kline 連動は Phase 8 スコープ（Phase 7 doc §3 では「Phase 8 で連動」と明記）

---

## 2. 確認項目チェックリスト

### 2.1 Rust 実装の呼び出しタイミング確認

**確認対象**: `src/ui/sidebar.rs` および `src/main.rs`

| チェック | 確認方法 | 期待する答え |
|---|---|---|
| `ListInstruments` gRPC がどのシステムから呼ばれているか | `sidebar.rs` / `main.rs` を grep | startup system か、LoadStrategy 完了イベント後か |
| `InstrumentList` Resource がいつ `Loading` → populated に遷移するか | `update_sidebar_system` の入力 Resource トリガーを確認 | LoadStrategy 完了後なら矛盾 A は「設計書が古い」で解決 |

**判定基準**:
- 起動時に呼んでいる → Phase 7 設計書の記述（「LOADED 後 1 回」）が古い。設計書を修正して完了
- LoadStrategy 後に呼んでいる → 「on startup に `1301.TSE` が表示」が別の理由による。原因を特定する

### 2.2 Backend の `ListInstruments` 実装確認

**確認対象**: `python/engine/server_grpc.py` の `ListInstruments` ハンドラ

| チェック | 確認方法 | 確認ポイント |
|---|---|---|
| `source="replay"` のときに何を参照しているか | `server_grpc.py` を読む | `catalog_path` 引数？ `SCENARIO.end_date`？ ハードコード？ |
| LOADED 遷移前（IDLE 状態）で呼ばれた場合の挙動 | コードと既存テスト `test_grpc_list_instruments.py` を確認 | エラーを返す？ 空リストを返す？ catalog_path ベースで返す？ |
| `source` パラメータが現在どう使われているか | proto と grpc handler を確認 | 未使用なら Phase 8 拡張時に壊れるリスクあり |

### 2.3 Phase 8 拡張への耐性確認

**確認対象**: `python/proto/engine.proto` の `ListInstrumentsRequest`

| チェック | 確認方法 | 期待する状態 |
|---|---|---|
| `source` フィールドが proto に存在するか | proto を読む | `string source = 1;` または相当フィールドがある |
| `source="live"` を追加したときに破壊的変更が起きないか | proto の field number と Rust 側の deserialize を確認 | 省略時にデフォルト値（`"replay"`）で動作する |

---

## 3. 期待する確認結果と対応方針

### シナリオ X（最良）: 実装は正しく、設計書が古い

- Rust 側は **起動時** に `ListInstruments` を呼んでいる
- backend は `catalog_path` で列挙できる銘柄を IDLE 状態でも返す
- `source` フィールドは proto に定義済みで、Phase 8 拡張で壊れない

**対応**: Phase 7 設計書の「`LoadStrategy` 完了後 1 回」という記述を「起動時」に修正。本計画書にその旨を記録して完了。

### シナリオ Y（要修正）: 実装と設計が乖離していて挙動が不定

- backend が IDLE 時に `ListInstruments` を呼ぶと何か問題がある（エラー or 意図しないデータ）
- Phase 8 の `source="live"` 拡張が現在の proto 定義で壊れる

**対応**: 最小限の修正（proto に `source` フィールド追加、backend の IDLE 時 fallback 追加など）を実施してから Phase 8 に進む。

---

## 4. 実施ステップ

```
Step 1 — Rust コード確認
  src/ui/sidebar.rs を読む
  src/main.rs で InstrumentList / ListInstruments の呼び出し箇所を確認

Step 2 — Python backend 確認
  python/engine/server_grpc.py の ListInstruments ハンドラを読む
  python/tests/test_grpc_list_instruments.py を読んで既存テストの前提を把握

Step 3 — Proto 確認
  python/proto/engine.proto の ListInstrumentsRequest / ListInstrumentsResponse を確認

Step 4 — 矛盾の判定
  各矛盾について「設計書が古い」か「実装が間違っている」かを判定する

Step 5 — 修正（シナリオ Y の場合のみ）
  最小限の修正を実施してテストが通ることを確認

Step 6 — 設計書更新
  Phase 7 設計書の矛盾した記述を修正
  本計画書に結果サマリーを追記して完了
```

---

## 5. 完了基準

- [ ] 矛盾 A（呼び出しタイミング）の真偽が確認済みで、設計書の記述が実装と一致している
- [ ] 矛盾 B（返却データのソース）が確認済みで、IDLE 時の挙動が明確に文書化されている
- [ ] 矛盾 C（Phase 8 移行）が確認済みで、`source` フィールドが proto に存在するか記録されている
- [ ] Phase 8 計画書の「Phase 7 ローカル固定銘柄一覧設計の廃止」が Phase 8 の実装ステップで確実に対処されることを確認（Phase 8 doc に `TODO` を追記）

---

## 6. 確認結果サマリー（実施後に記入）

<!-- 実施後にここを埋める -->

### 矛盾 A
- 呼び出しタイミング: TBD
- 判定: TBD

### 矛盾 B
- backend の返却ソース: TBD
- IDLE 時の挙動: TBD

### 矛盾 C
- proto `source` フィールド: TBD
- Phase 8 拡張耐性: TBD

### 総合判定
- シナリオ X / Y: TBD
- 実施した修正: TBD
