---
name: parallel-agent-dev
description: 実装計画をタスクグラフに分解し、依存関係の無いタスクを複数エージェントに並行させて消化するオーケストレーション手法。大型フェーズ（5タスク以上）を単一エージェントで順次実装するより 3〜5倍速で完遂できる。トリガー: 「/parallel-agent-dev」「大型フェーズ」「フェーズを実装して」「計画書を実装して」「複数言語にまたがる実装」「Python + Rust 実装」「プランを並列で」「並行実装」「複数エージェント」。ユーザーが /pair-relay を指定しても対象が 10 タスク以上 / Python+Rust 混在の場合はこちらを優先する。
---

# Parallel Agent Dev — 分担並行実装オーケストレーション

大型の実装フェーズを、複数の専門エージェントに分担させて並行実装する手法。

```
/parallel-agent-dev
```

オーケストレーター（あなた）がタスクグラフを把握し、**依存のないタスクを同一メッセージで並列起動**、**依存があるタスクは完了待ち後に直列起動**する。

---

## なぜこの手法か

- **速度**: 独立タスクは並列で同時進行。5 フェーズを直列で回すと 5 倍かかるが、依存グラフ次第で 2〜3 ステップに圧縮できる
- **コンテキスト汚染防止**: 各エージェントは自分の担当タスクだけを文脈に持つ。一人のエージェントに全タスクを与えるとコンテキスト消費が爆発する
- **専門化**: 「Bevy ECS システム」「Python gRPC サーバ」「Rust gRPC クライアント」など責務が明確に分離されたタスクは、それぞれ担当エージェントが深く集中できる
- **進捗の可視化**: 各エージェントが `implementation-plan.md` に ✅ を付けることで、オーケストレーターが進捗を即座に把握できる

---

## 不可侵ルール

- **TDD 厳守**: 全エージェントが `.claude/skills/tdd-workflow/SKILL.md` に従う
- **テスト常時緑**: `cargo test --workspace` と `uv run pytest python/tests/` を各エージェント完了時に確認
- **Clippy クリーン**: `cargo clippy -- -D warnings` をクリーンに保つ
- **進捗記録**: 完了タスクに ✅ を付け、設計判断を計画書に随時追記
- **prompt は self-contained**: サブエージェントは前会話を見ない。必読ファイルパスと背景をプロンプトに全て含める

---

## 各エージェントの行動指針

各サブエージェントは以下の行動指針を必ず守ること。

### 1. 着手前に目的・制約・完了条件を確認する

- **Goal（目的）／Constraints（制約）／Acceptance criteria（完了条件）** が明確でなければ作業を進めず、オーケストレーターに再確認すること。
- プロンプトに記載がない場合は「どれが完了条件ですか？」と問い返す。

### 2. 不確かな情報を回答に含めない

- **事実確認が取れない情報、不確かな情報、または推測に基づいた情報は絶対に回答に含めないこと。**
- 確証がない場合は「確認が取れていません」と正直に伝え、確認手段を示す。

### 3. 計画書への進捗書き込み

- 進捗があり次第、計画書（`docs/plan/Phase N - *.md` 等）に **状況・新たな知見・設計思想と背景・Tips** など他の作業者に必要な情報を随時書き込む。
- 完了した作業項目には **✅** を付けて進捗を共有する。
- 設計上の判断（「なぜこの実装にしたか」）も記録する。

### 4. TDD アプローチ（`.claude/skills/tdd-workflow/SKILL.md`）

- 実装は必ず `.claude/skills/tdd-workflow/SKILL.md` に従った TDD サイクル（RED → GREEN → REFACTOR）で進める。
- テストを先に書き、失敗を確認してから実装する。

### 5. 実装完了後にレビュー＋修正（`./ImplementationLoop.md`）

- 担当タスクの実装が完了したら、`.claude/skills/parallel-agent-dev/ImplementationLoop.md` を起動してコードレビューと修正を実施する。
- MEDIUM 以上の指摘がゼロになるまでループを継続してから「完了」を報告する。

---

## ステップ 0 — 計画書を読んで依存グラフを把握する

オーケストレーターは必ず以下を先に読む:

1. `docs/plan/Phase N - <feature>.md` — タスク一覧と完了条件
2. 関連する architecture doc / open-questions

読んだ後、**タスク間の依存関係を手元でまとめる**:

```
例（Phase 7 相当）:
  A (Bevy UI コンポーネント)  ─┐
  B (Python gRPC エンドポイント) ─┼─→ C (Rust gRPC クライアント) ─→ D (統合テスト)
  (独立)                       ─┘
```

ポイント:
- 「B の proto 定義を C が使う」→ proto 凍結後に C を起動
- 「A と B はファイルが重ならない（`src/ui/` vs `python/engine/`）」→ A と B は並列可
- 「D は A/B/C 全ての上に乗る」→ D は A/B/C が全完了してから

---

## ステップ 1 — エージェント分割の原則

### 分割の粒度

| 状況 | 方針 |
|---|---|
| ファイルが重ならない | 迷わず並列 |
| 一方が他方の出力（proto・型・フォーマット）を使う | 直列（後者を先に完了させる） |
| 同一ファイルを両方が編集する | 直列か、ファイルを分担（先者がインターフェースを確定してから後者に渡す） |
| Bevy UI と Python バックエンド | proto 型が凍結されていれば並列可 |

### エージェントへの指示テンプレ

各エージェントへのプロンプトは以下を含む（self-contained 必須）:

```
あなたは [担当タスク名] 担当サブエージェントです。

## 行動指針（必ず守ること）
1. **Goal/Constraints/AC の確認**: 目的・制約・完了条件が不明な場合は作業を開始せず再確認する。
2. **不確かな情報の禁止**: 事実確認が取れない情報・推測は回答に含めない。確証がなければ正直にその旨を伝える。
3. **計画書への進捗書き込み**: 進捗・知見・設計判断・Tips を随時 docs/plan/Phase N - *.md に書き込み、完了項目に ✅ を付ける。
4. **TDD**: `.claude/skills/tdd-workflow/SKILL.md` に従い RED→GREEN→REFACTOR サイクルで実装する。
5. **レビュー＋修正**: 実装完了後に `.claude/skills/parallel-agent-dev/ImplementationLoop.md` を起動し、MEDIUM 以上の指摘がゼロになるまでループしてから「完了」を報告する。

## 必読ファイル（最初に読むこと）
- [計画書パス] — [該当セクション]
- [実装対象ファイルパス]

## 前提情報（他エージェントから引き継いだ仕様）
[依存する成果物の仕様サマリ（proto 定義・型・API シグネチャ等）]

## 担当タスク
[具体的な実装内容（箇条書き）]

## テスト要件（TDD）
[書くべきテストとその完了条件]

## 完了条件
- cargo test --workspace が全緑
- uv run pytest python/tests/ -v が全緑
- cargo clippy -- -D warnings がクリーン
- docs/plan/Phase N - *.md の [タスク名] に ✅ を付ける
- ImplementationLoop.md で MEDIUM 以上の指摘がゼロであること

## 注意事項
[不変条件・禁止事項・他エージェントとの境界]

完了したら「[タスク名] 完了」と報告してください。
```

---

## ステップ 2 — 並列起動（同一メッセージで複数 Agent ツール呼出）

依存のないエージェントは **同一メッセージ内に複数の Agent ツール呼出**を並べる。
順次起動するとコスト・時間が無駄になる。

```
# 正しい（並列）
Agent(A の prompt) と Agent(B の prompt) を同一メッセージで呼出

# 誤り（直列）
Agent(A) を呼出 → 完了待ち → Agent(B) を呼出
```

**`isolation: "worktree"` の注意**: フィーチャーブランチ作業中は worktree が `main` から作られ、ブランチ上の新設ファイルが存在しないケースがある（知見 #12 参照）。フィーチャーブランチ上では worktree の代わりに「ファイル単位の担当分け」で並行性を確保する。

---

## ステップ 3 — 完了待ちと引き継ぎ

依存グラフの「親」が全て完了したら子を起動する。

### 引き継ぎ情報の受け渡し方

エージェントが生成する成果物（proto 定義・API シグネチャ・型定義）を、次のエージェントのプロンプトに **直接埋め込む**:

```
# 親エージェント（B）の完了報告から抽出:
「proto: rpc StartEngine(StartEngineRequest) returns (StartEngineResponse)」

# 子エージェント（C）のプロンプトに埋め込む:
## 前提情報: B が確立した gRPC インターフェース
message StartEngineRequest { string strategy_file = 1; }
message StartEngineResponse { bool ok = 1; string error = 2; }
```

ドキュメントへの追記で引き継ぐこともできる（B が計画書を更新 → C がそれを読む）が、プロンプト直接埋め込みの方が確実で速い。

---

## ステップ 4 — 中間検証（各直列ステップの境界で実施）

並列グループが完了するたびに、オーケストレーターが **全テストを手元で叩いて確認**:

```bash
cargo test --workspace 2>&1 | grep -E "^(test result|FAILED|error)"
uv run pytest python/tests/ -q 2>&1 | tail -5
```

エージェントの「全緑」主張を鵜呑みにせず、自分で確認する。ここで失敗が見つかったら次グループを起動する前に修正する。

---

## ステップ 5 — 全完了後の最終検証

全エージェントが完了したら:

```bash
cargo test --workspace
cargo clippy -- -D warnings
uv run pytest python/tests/ -v
```

完了したら `ImplementationLoop.md` でコードレビューを走らせる（大型フェーズは必須）。

---

## 典型的な依存グラフパターン

### パターン A: 独立並列 → 合流 → 直列

```
A ─┐
B ─┼─→ C → D → E
C ─┘
```

- A / B は互いに独立 → 並列
- C は A + B の成果物を使う → A + B 完了後
- D / E は順次依存 → 各完了後

### パターン B: Bevy UI / Python バックエンド の並列（proto 型凍結前提）

```
proto 凍結（事前完了）
       ↓
Bevy UI 実装 ─┐
Python gRPC 実装 ─┴─→ 統合テスト（tests/backend_integration.rs + python/tests/）
```

`python/engine/proto/engine.proto` の型（message / enum）が凍結されていれば、
Bevy 側 Rust クライアント (`src/trading.rs` 等) と Python サーバ実装は独立して並列できる。

### パターン C: フェーズゲート（完了条件が次フェーズのブロッカー）

```
Phase 6 ─→ Phase 7 ─→ Phase 8
```

各フェーズに「Exit 条件」があり、それを満たさないと次フェーズに進めない場合。
フェーズ内タスクは可能な限り並列化し、フェーズ間だけ直列にする。

---

## エージェント分割の目安

| タスク規模 | 分割方針 |
|---|---|
| 1〜2 ファイル変更 | 単一エージェントで OK |
| 3〜5 ファイル（同一言語・同一責務） | 単一エージェントで OK |
| 5〜10 ファイル（異なる責務・言語混在） | 2〜3 エージェントに分割 |
| 10 ファイル以上 / 複数フェーズ | 必ず分割。フェーズ単位または責務単位で |

---

## 失敗パターン（避けること）

1. **エージェントをすべて直列起動する** — 最大で N 倍の時間がかかる。依存がなければ並列
2. **「前の会話を見てください」と書く** — サブエージェントは会話履歴を見ない。self-contained なプロンプト必須
3. **同一ファイルを複数エージェントに割り当てる** — マージ競合の元。ファイル単位で担当を分ける
4. **親エージェントの出力をプロンプトに含めない** — 子エージェントが仕様を知らず互換性のない実装をする
5. **中間検証を省く** — 後続エージェントが壊れた基盤の上に実装し、最後になって発覚する
6. **完了報告の鵜呑み** — `cargo test --workspace` を自分で叩いて確認する
7. **フィーチャーブランチで isolation: "worktree" を使う** — main から worktree が作られ新設ファイルが存在しない（`ImplementationLoop.md` 知見 #12 参照）
8. **計画書の全タスクをエージェントに割り当てたか確認しない** — タスク分解後、計画書の全項目を「どのエージェントが担当するか」チェックリストで確認する。見落としは Round 3 の単独 agent になる（Phase 8.7 で D26 menu_bar 追加が漏れた実例）
9. **Bevy system chain の 20-tuple 上限を忘れる** — `add_systems(Update, (sys1, sys2, ...).chain())` の tuple は最大 20 要素。§5.3 の順序指定で 18+ systems になる場合は複数 chain を `.after()` で繋ぐ形に分割する

---

## parallel-agent-dev vs pair-relay の使い分け

| 状況 | 推奨 |
|---|---|
| 大型フェーズ（10+ タスク、Python + Rust 混在） | **parallel-agent-dev** — ファイル単位で分割し 3〜4 agents 並列 |
| 単一ファイルの逐次的な変更、または Navigator の設計判断が必要な場面 | **pair-relay** |
| ユーザーが `/pair-relay` を指定したが対象が大型フェーズの場合 | parallel-agent-dev に自動昇格し、pair-relay は不使用で OK |

## 実績

Phase 7（Replay UI Integration）を 4 エージェント並列で実装した例:

| ステップ | 並列エージェント | 直列待ち |
|---|---|---|
| 1 | proto 凍結（単独） | — |
| 2 | A（Bevy UI コンポーネント）/ B（Python gRPC エンドポイント）並列 | proto 凍結後 |
| 3 | C（Rust gRPC クライアント `src/trading.rs`）| A + B 完了後 |
| 4 | D（統合テスト）| C 完了後 |

Phase 8.7（Unify Sidebar Instruments and Tickers）を 7 エージェント 3 ラウンドで実装した例:

| ラウンド | 並列エージェント | 直列待ち |
|---|---|---|
| 1 | PY-A（server_grpc + core + last_price_cache）/ PY-B（scenario.py）/ RS-A（trading.rs + main.rs）/ RS-B（components.rs writeback gate）並列 | — |
| 2 | RS-C（scenario_parser + prune）/ RS-D（sidebar + picker）/ RS-E（restore.rs）並列 | Round 1 全完了後 |
| 3 | 1 agent（mod.rs wire-up + menu_bar D26 + Step 12 E2E）| Round 2 全完了後 |

---

## このスキル自体のメンテナンス

新フェーズで適用した後、新しい知見があれば `ImplementationLoop.md` の「知見（実績ベース）」セクションに追記する。
