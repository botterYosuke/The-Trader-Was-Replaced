# ImplementationLoop — ソースコードレビュー・修正ループ

ソースコード（Rust / Python 実装ファイル）のレビュー・更新に使う。
共通ルール・収束基準は [`SKILL.md`](./SKILL.md) を参照。

---

あなたはオーケストレーターです。
実装ファイル群（Rust / Python）に対して「レビュー → 集約 → 修正 → 検証」を 1 ラウンドとし、MEDIUM 以上の Finding がゼロになるまで反復させます。

## 収束の期待値（実測ベース）

中規模フェーズ（30 ファイル前後の Rust + Python）の典型的収束カーブ:

| ラウンド | CRITICAL+HIGH+MEDIUM 件数 | 説明 |
|---|---|---|
| R1（初回） | 25–30 | 設計層・サイレント・型・IPC が重複指摘される |
| R2 | 10–15 | 初回 fix 後、新規導入された軽微な問題が中心 |
| R3 | 5–7 | 残存 MEDIUM、コメント整合・テスト品質 |
| R4（収束） | 0 | サニティチェックのみ |

**1 ラウンドで収束することはほぼない。3–4 ラウンドを見積もる。** 件数が半減せず横ばいなら指示が曖昧で fix が浅い兆候。

## 起動チェック

ラウンド 1 開始前に必ず以下を実行する:

1. レビュー対象の計画書・規約を**必ず先に読む**:
   - 該当フェーズの計画書（例: `docs/plan/Phase 7 - Replay UI Integration.md`）
   - 関連アーキテクチャ doc / open-questions
   - `.claude/skills/tdd-workflow/SKILL.md`
2. 現状の build/test 状態を実コマンドで確認。レビュアーが「全緑」と主張しても自分で叩いて裏を取ること

## ループ手順

### Step 1: レビュー（サブエージェント並列）

以下のサブエージェントを **同一メッセージ内で並列起動**（独立タスクは並列が原則）:

| エージェント | 観点 |
|---|---|
| `rust-reviewer` | 所有権・ライフタイム・unsafe・エラー処理・`?` 伝播 |
| `silent-failure-hunter` | 握り潰しエラー・creds 漏洩・ログ不足・`unwrap` 地雷 |
| `bevy-ecs-reviewer` | ECS 設計逸脱（システム順序・リソース競合・イベント取りこぼし）※ Bevy 変更時のみ |
| `type-design-analyzer` | Newtype・状態機械・enum 不変条件 |
| `grpc-compatibility-auditor` | proto スキーマ整合・gRPC エラーコード・破壊的変更 ※ proto/gRPC 変更時のみ |
| `general-purpose` | Python コード品質（Nautilus 規約・型ヒント）+ 計画書クロスチェック |

各エージェントへの指示テンプレ（self-contained 必須）:

> `docs/plan/` 配下のドキュメントを必ず参照し、実装が計画と整合しているか検証せよ。指摘は **CRITICAL / HIGH / MEDIUM / LOW** で分類し、`path:line`、根拠（計画書のどの条項に違反か）、推奨修正、回帰防止テストの提案を含めよ。末尾に重要度別件数サマリ。500 行以内。

Bevy UI を含まない Python/gRPC 変更なら `bevy-ecs-reviewer` を省略してよい。proto 変更なしなら `grpc-compatibility-auditor` も省略。**スコープに合わせてエージェントを選ぶ**。

### Step 2: 集約（オーケストレーター本人）

全エージェントの指摘をマージし、重複統合 → 重要度順に並べた一覧を作成。CRITICAL / HIGH / MEDIUM の件数を要約。

**集約時の注意**:
- 同じ問題が複数エージェントから別 ID で報告されることが多い。**高い方の重要度を採用**
- レビュアー間で重要度判断が割れた場合は、production リスクが高い方を採用

### Step 3: 修正（サブエージェント並列）

**MEDIUM 以上が 1 件でもあれば** `general-purpose` エージェントに修正依頼。

> **`implementer` サブエージェントは単一 RED→GREEN サイクル制約があり、大きな batch を拒否する。** 多項目を一括で進めたいときは `general-purpose` に「TDD 順序で順次着手せよ」と明示する。1 項目ずつ厳密に進めたい場合は test-writer → implementer のペアを項目ごとに回す。

修正エージェントへの指示には必ず以下を含める:

- 該当ファイル・行・指摘内容（オーケストレーター側で要約）
- 不可侵ルール一式（[`SKILL.md`](./SKILL.md) 参照）
- TDD 順守と各項目の RED → GREEN → REFACTOR 順序
- **uv 環境利用の明記**（Python 関連は `uv run pytest`、`uv run python -m engine`、`uv add` 必須。素の `python` 禁止）
- **「降格判断はユーザー権限。実施できないと判断したら DEFER ではなく STOP+REPORT して指示を仰げ」と明示**
- **「対象ファイル外を変更しない」**: 修正対象として列挙したファイル以外は触らない
- 修正後の最終コマンド緑確認（`cargo fmt --check` も含む）
- **計画書の該当フェーズ末尾に「レビュー反映 (YYYY-MM-DD, ラウンド N)」ブロックを追記**

修正項目は依存関係順にグループ化する（例: 型変更 → サーバ実装 → クライアント実装 → テスト）。**型シグネチャ変更や proto 変更は最初に実施**（後続項目への影響を吸収しやすい）。

### Step 4: 修正の検証と再レビュー

修正後にレビュー段階を再実行。**ただし全エージェントを毎回回す必要はない**:

- ラウンド 2 以降は **変更があった層のレビュアーのみ**（例: Python だけ変えたなら silent-failure-hunter + general-purpose、Bevy ECS 変更なら bevy-ecs-reviewer）
- **silent-failure-hunter は毎回必ず回す**: 「fix が新たな silent failure を導入する」パターンが頻出

### Step 5: 次ラウンドへ / ループ終了

CRITICAL/HIGH/MEDIUM が残存する場合は Step 1 に戻る。次ラウンドのレビュー指示には:
- **当ラウンドで修正された箇所を重点検査**する旨を明記
- ラウンド数が増えたら投入エージェントを絞る（変更層のみ + silent-failure-hunter 固定）

## 出力形式（毎ラウンド）

各ラウンド開始時:

```
=== ラウンド N ===
残存 CRITICAL: X件 / HIGH: Y件 / MEDIUM: Z件 / LOW: W件
```

計画書の該当フェーズ末尾に **「レビュー反映 (YYYY-MM-DD, ラウンド N)」** ブロックを追記し続ける:

- 完了項目に ✅
- 設計判断・新たな知見・Tips を他作業者が再現できる粒度で

書く内容:
1. 解消した指摘（id + 1 行サマリ）
2. 修正中に発覚した設計判断（plan を更新する根拠）
3. 新たな見逃しパターン候補
4. 持ち越し項目とその理由

**サイズ管理**: 各ラウンド反映ブロックが肥大化する。「ラウンド N で解消」と書いた項目は次ラウンド以降では繰り返し書かない。

## 完了サマリ

```
=== 完了 ===
全ラウンド数: N
修正した Finding 総数: CRITICAL X / HIGH Y / MEDIUM Z / LOW W
残存 LOW（対応不要）: K件
主要な反映成果:
- 型安全: ...
- silent failure 除去: ...
- テスト追加: M件（cargo test / pytest 緑確認済）
- proto / gRPC 整合: ...
```

## ループ上限と escape hatch

- **最大 N ラウンド = 8** をハード上限とする。それを超えても収束しない場合は強制終了し、残存 CRITICAL/HIGH/MEDIUM を計画書の open-questions セクションに **未決オープン質問として書き出す**
- CRITICAL/HIGH/MEDIUM 件数が **3 ラウンド連続で減らない**場合、投入レビュアーの観点が実装スコープとずれているサインなのでユーザーに相談する

## オーケストレーター運用 Tips

### 並列起動

> 独立タスクは **同一メッセージ内で複数 Agent 呼出**。「6 件並列」＝ 1 メッセージで 6 ツール呼出。順次起動するとコストも時間も無駄。

### バックグラウンド実行

レビューエージェントは長時間（数十秒〜数分）かかるため `run_in_background: true` で投入し、完了通知を待つ。Sleep ループは禁止。

### 修正範囲の判断

| 発見 | 対応 |
|---|---|
| CRITICAL | 必ず即修正 |
| HIGH（コード変更） | 同 PR で修正 |
| HIGH（大規模リファクタ・別 PR スコープ） | **ユーザーに承認を取る**。承認後に計画書「繰越」に明示してパス |
| MEDIUM | 同 PR で修正（このスキルの停止条件） |
| LOW | 列挙のみ。次フェーズで拾うかどうかをユーザーに判断してもらう |

### `implementer` vs `general-purpose`

- `implementer`: **1 項目厳密 TDD**。RED テストの handoff が必須。多項目を投げると拒否される
- `general-purpose`: **多項目 batch + TDD 順守可**。プロンプトで「各項目で RED→GREEN→REFACTOR」と明示する

### コミット時の選択的ステージング

修正エージェントが `cargo fmt --all` を実行すると、フェーズと無関係なファイルにもフォーマット差分が出る。
コミット時は `git add -A` を避け、フェーズに関連するファイルを **明示列挙**してステージング。

---

## 禁止事項と失敗パターン

### ループ固有の禁止事項

- `silent-failure-hunter` を省略してはいけない。fix が新たな silent failure を生む頻度が高い
- 修正エージェントに勝手に繰越を決めさせてはいけない。降格はユーザー権限
- subagent の「全緑」主張を鵜呑みにしてはいけない。**必ず `cargo fmt --check` 等を自分で叩いて裏を取る**

### 失敗パターン（避けること）

1. **MEDIUM を無視して LOW だけ残した状態で「完了」にする** — ループ条件違反。MEDIUM ゼロまで繰り返す
2. **修正後の再レビューをスキップ** — 修正で新規 MEDIUM が混入していないか必ず確認する
3. **全エージェントを順次起動** — 並列が原則
4. **修正エージェントを `implementer` で多項目投げる** — 拒否されて時間ロス
5. **計画書追記を最後にまとめる** — ラウンドごとに追記しないと次のレビュアーが「何が解消済みか」判断できない
6. **fix 後に silent-failure-hunter を回さない** — fix 由来の新規 silent failure を見落とす
7. **subagent の「全緑」主張を鵜呑み** — 自分で `cargo fmt --check` 等を叩いて裏を取る
8. **コミット時に `git add -A` を使う** — 別フェーズの作業や untracked artifact が混入

---

## 知見（実績ベース）

### 1. subagent の勝手繰越癖

ユーザーが「全件修正」と明示しても、修正エージェントは「影響範囲が大きい」等の理由で独断で次フェーズへ降格することがある。**プロンプトに「降格判断はユーザー権限。困ったら STOP+REPORT」を明記**するまでこの癖は再発する。

### 2. fix 自体が silent failure を生む

修正は新たな silent failure を生む。**silent-failure-hunter は毎ラウンド必ず回す。**

### 3. `#[doc(hidden)] pub` ≠ `#[cfg(test)]`

test-only API を `#[doc(hidden)] pub fn ...` にしても **production バイナリに symbol が残る**。外部クレートから呼べる。`tests/` は外部クレート扱いなので `#[cfg(test)]` だと呼べない。**正解は `#[features] testing = []` + self dev-dep で feature-gate**。

### 4. Newtype を作ったら `From` 実装を慎重に削る

`InstrumentId(String)` を作っても `From<String>` / `From<&str>` を残すと newtype の意図（誤代入のコンパイル検知）が無効化される。**newtype 導入時は `From<inner>` を削除し、`new(impl Into<inner>)` 一本化**。

### 5. リスナー / spawn の JoinHandle 捨て

`tokio::spawn(async move { ... })` で `JoinHandle` を捨てると、再起動時に新旧 listener が同一チャネルを購読する窓ができる。**spawn handle は `Mutex<Option<JoinHandle>>` で保持し、再 spawn 前に `abort().await`**。

### 6. コメントと実装の乖離

`// removed: ...` というコメント直下に実装が残るケースがある。**grep `"dropped:" "removed:" "deleted:"` 等のキーワードで該当箇所を機械抽出**して最終レビューで確認。

### 7. 正規表現ベースのソース検査は脆い → AST へ

ソースコード解析テストは `re.search` より `ast.parse` + visitor（`Assign` / `AnnAssign` / `NamedExpr` を網羅）が堅牢。

### 8. テスト sentinel と `.env` の値衝突

`.env` の dev creds とテストの漏洩検知 sentinel が同一文字列になると検知が無効化される。**test sentinel は `TEST_SENTINEL_<uuid8>` 形式で realistic value とは交わらないドメインに置く**。

### 9. `--token` CLI 引数 = secrets leakage

`argparse` で `--token VALUE` を受けると `ps -ef` の commandline 列に値が残る。**stdin 経路に統一し、CLI flag は `argparse.SUPPRESS` で隠して deprecation warning**。

### 10. cargo fmt の workspace 一括は無関係ファイルを汚す

`cargo fmt --all` は workspace 全体に走るため、フェーズと関係ない `src/ui/*.rs` まで diff が出る。コミット時に「fmt 由来か機能変更由来か」を `git diff --stat` で確認する。

### 11. proto 破壊的変更の検知漏れ（gRPC 固有）

`message` フィールドのリナンバリング（field number 変更）は wire compat を壊すが、protoc はエラーにしない。**proto 変更時は `grpc-compatibility-auditor` が field number の変更履歴を確認**し、既存クライアントへの影響を明示する。

### 12. `isolation: "worktree"` がフィーチャーブランチで base 不整合を起こす

フィーチャーブランチ作業中に `isolation: "worktree"` を使うと、worktree が **`main` から作られ**、ブランチ上で新設したファイル（例: `python/engine/replay_session.py`）が存在しない状態になる。エージェントが「対象ファイルが無い → 推測で実装するのは事故」と正しく判断して STOP+REPORT する。

**対策**: フィーチャーブランチ作業中は **worktree を使わずメインリポジトリで直接作業**させる。並行性は「ファイル単位の担当分け」で確保し、計画書のような共有ファイルだけ orchestrator が最後に集約する。

### 13. 単一エージェント batch の限界

CRITICAL 6 / HIGH 10 / MEDIUM 16 + 大型新機能 + テスト分割 を **1 体の `general-purpose` に投げると着手前に STOP+REPORT** される。理由は「TDD 厳守 + 全検証緑で時間予算が読めない」。

**閾値の経験則**:
- 〜10 項目 + 軽い機能 → 単一 `general-purpose` で OK
- 10〜30 項目 → 依存順 batch に明示分割しても単一 agent でいける場合あり
- **30 項目超 / 大型新機能含む → `/parallel-agent-dev` 必須**

### 14. 計画書末尾「レビュー反映」ブロックの並行更新競合

Rust エージェントと Python エージェントを並行実行する際、両者が同じ計画書末尾に追記しようとすると競合する。

**対策**: 並行する agent のうち **1 つだけ計画書の追記責任を持たせ、他は完了報告に差分サマリを箇条書きで返す**。orchestrator が最後に集約してまとめる。

### 15. fix 自体が silent failure を生む（定量的傾向）

| ラウンド | 新規 silent failure 発見 | 前ラウンド fix 由来 |
|---|---|---|
| R1 (初回レビュー) | 25〜32 件 | (実装由来) |
| R2 サニティ | 8 件前後 | R1 fix 由来が大半 |
| R3 サニティ | 4 件前後 | R2 fix 由来が大半 |
| R4 サニティ | 0 件（収束） | — |

**毎ラウンド `silent-failure-hunter` を必ず投入**し、前ラウンドで導入した新変数・新フィールド・新 if 分岐をピンポイントで検査する prompt にすると効率的。

### 16. Bevy ECS — システム実行順序の暗黙依存

Bevy の `App::add_systems` で順序指定なしにシステムを追加すると、**同一スケジュール内の実行順が非決定的**になる。特に「gRPC レスポンスを受け取るシステム → UI を更新するシステム」のような依存がある場合、`.before()` / `.after()` または `SystemSet` での明示的な順序付けが必要。`bevy-ecs-reviewer` がこの見落としを重点チェックする。

### 17a. レビュアー名は agent type ではなく role prompt（spawn は general-purpose）

Step 1 の表に並ぶ `rust-reviewer` / `silent-failure-hunter` / `type-design-analyzer` / `grpc-compatibility-auditor` は **登録された subagent_type ではない**（環境の Agent ツールに無い）。これらは「観点（role）」であり、実体は **`general-purpose` サブエージェントに role 固有のプロンプトを渡して spawn** する（read-only 徹底なら `Explore` でも可）。プロンプト側で「あなたは silent-failure-hunter の観点でレビューせよ／握り潰しエラー・creds 漏洩・ログ不足・unwrap を重点に」と役割を明示すれば、専用 agent type が無くても同等の分業ができる。Python のみの変更なら `bevy-ecs-reviewer`/`rust-reviewer`/`grpc-compatibility-auditor` は spawn せず、silent-failure + type-design + 言語別品質（kabu/tachibana なら該当 venue スキルを読ませた general-purpose）に絞る。

### 17b. 大きいファイルの単一 Read は Edit の exact-match に使うな

数百行を 1 回の Read で読むと、特定行（関数本体）が**不正確に再構成**されることがある（Phase 9 Step 6 レビューで `_send_order` の戻り値型・`fetch_account` の `asyncio.gather` を初回 full read が取り違えた実例）。`Edit` の `old_string` を作る直前は**対象関数を狭い offset/limit で再 Read** して現物を確定させる。`String to replace not found` が出たら記憶ではなく再 Read を信じる。

### 18. websockets `async for` vs `ws.recv()` の挙動差異

websockets 10+ では `async for raw in ws:` でループするコードが `ConnectionClosedOK` を `StopAsyncIteration` に変換し、`except ConnectionClosedOK` ブロックに到達しない。無限高速再接続が発生する。

**修正パターン**: `ws.recv()` を直接呼び出す + `asyncio.wait_for` でタイムアウトを設ける。**reviewer 観点**: WebSocket 受信ループを `async for` で書いたコードは、websockets バージョンと照合して `except ConnectionClosedOK` が実際に到達可能か確認する。

### 19. レビュー修正中にユーザー（or 別セッション）が同じファイルを並行編集する

review→fix ループ中に、ユーザーが**レビュー対象ファイルを並行で commit / 編集**することがある（実例: Phase 9 Step 6 のレビュー修正中に、ユーザーが Step 6 を commit して同じ `kabusapi.py` に Step 7 の `check_health` を書き足した）。兆候は ① `<system-reminder>`「ファイルが変更された」通知、② `git diff --stat` の行数が**減る**（HEAD が進んだ＝ commit された）、③ 自分が書いていない関数/定数が出現する。

**これを「自分の編集が壊された/corruption」と早合点しない**（知見 #6/#10 の延長）。対処:
- `git log --oneline -3` で HEAD が進んだか、`git status --short` で何が uncommitted かを確認する。`git diff --stat` の縮小は多くの場合 commit によるもので破損ではない。
- **自分の編集が生存しているかは `grep` で実地確認**する（追加した定数名・関数名・reject_reason 文字列を数える）。diff-stat や reminder の本文だけで判断しない。
- 以後の `Edit` は**直前に対象関数を狭く再 Read**（#17b）してから行う。並行編集と領域が重ならなければ exact-match Edit はそのまま通り、重なれば `old_string not found` で安全に失敗する（clobber しない）。
- **全体テストは並行編集で moving target になる**。自分の担当スコープ（触ったファイルのテスト）に絞って緑を確認し、ユーザーの in-progress 変更（別 step）は自分の収束判定に含めない。
- 共有ファイル（計画書 doc 等）への「レビュー反映」追記は、ユーザーが別 step セクションを書いていても**領域の重ならない自セクション末尾**に surgical に入れる。

### 20. 「レビューして」と言われたとき、未コミットツリーが既に *前回の review-remediation ラウンド* のことがある

「Phase N 完成、レビューして」の依頼で working tree を見ると、**コミット済み実装の上に未コミットの review-fix delta が積まれている**ことがある（実例: Phase 9 — Step 0–10 はコミット済みだが、`git diff` が `Finding 1/3` / `MEDIUM-1..5` / `fix #3/#4/#5` といった**レビュー由来のコメント付き差分** +1200 行を含んでいた）。これは「新規実装をゼロからレビュー」ではなく「**前回の修正ラウンドの検証 + fix 由来の回帰再レビュー**」が正しいフレーミング。

兆候: `git diff` のコメントに `Finding`/`MEDIUM-N`/`review fix`/`item N`/`fix #N` 等のレビュー語彙、`git log` は当該フェーズが既にコミット済み（HEAD は次フェーズ）。[[crash-recovery-diagnose-before-resume]] と同根。

対処:
- **コミット済み全体を再レビューし直さない**（多ラウンド収束済みの指摘を蒸し返すだけ）。**未コミット delta を一次対象**にし、各 fix を「正しいか / 新たな silent-failure を生んでいないか」で読む。
- 1 周目レビューの prompt に「この delta は前回の修正。fix の正しさと fix 由来の回帰を見よ」と明記する。
- fix が**逆方向**に書かれている指摘を鵜呑みにしない（実例: bevy reviewer が Escape 順序を `.after` 推奨と書いたが、interleaving trace では `.before` が正だった。#6「鵜呑み禁止」の延長 — 順序系の指摘は自分で trace してから適用）。
- ベースライン緑は**自分で**取る（`cargo test`/`pytest` の pre-existing failure を delta 由来と誤認しない。Phase 9 では Windows pipe-FD の `test_grpc_shutdown`×3 等が恒常 baseline）。

### 21. 順序ガード（`Res<Flag>` を読んで早期 return）の system は、フラグを書く system との `.before`/`.after` を必ず固定する

「上位モーダルが開いていれば Escape を無視する」式の cross-talk ガードは、**ガードを読む側がフラグを更新する側より前に走らないと非決定的**になる（実例: Phase 9 §3.10 — SecretModal は Escape を `Events<KeyboardInput>::drain()` で消費、通知モーダルは `ButtonInput::just_pressed` で読む別チャネル。順序制約が無いと scheduler 順次第で 1 回の Escape が両方を閉じる）。**per-system の単体テストはフラグを手で立てるので誤順序でも通り、回帰を捕まえない**。対策: ガード読み手を `.before(フラグ書き手)` に固定し、**両 system を本番順序で組んで実入力を 1 回流す schedule レベルのテスト**を 1 本足す（`.before`→`.after` flip と cycle の両方を検出）。

---

## 汎用呼び出しテンプレート

新フェーズ・PR を仕上げるときにオーケストレーター（あなた）に貼り付けて使う。`{{}}` プレースホルダーを実値に置換すること。

---

あなたは **オーケストレーター** です。`The-Trader-Was-Replaced` リポジトリで `{{feature}}` のフェーズ `{{phase_id}}` 「`{{phase_title}}`」を レビュー → 修正のループで仕上げてください。

**唯一のリファレンス**: すべての手順・不可侵ルール・収束基準・既知の落とし穴は [`SKILL.md`](./SKILL.md) と本ファイル（[`ImplementationLoop.md`](./ImplementationLoop.md)）に集約されています。

### 必読ドキュメント

```text
{{plan_doc}}              # 例: docs/plan/Phase 7 - Replay UI Integration.md
{{spec_doc}}              # 例: docs/strategy-replay.md
.claude/skills/tdd-workflow/SKILL.md
```

### レビュー対象スコープ

```text
{{file_list}}             # 例:
                          # src/trading.rs
                          # src/ui/components.rs
                          # python/engine/server_grpc.py
                          # python/engine/core.py
                          # python/engine/proto/engine.proto
```

### プロジェクト固有の検証コマンド

```bash
cargo check --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check
cargo test --workspace
uv run pytest {{test_glob}} -v
# 例: uv run pytest python/tests/ -v
```

### 起動するレビュアー

```text
{{reviewers}}
# デフォルト推奨セット（フルスタック変更時）:
# rust-reviewer, silent-failure-hunter, bevy-ecs-reviewer,
# type-design-analyzer, grpc-compatibility-auditor, general-purpose
```

### スコープ外（subagent が触らないこと）

```text
{{out_of_scope_paths}}
# 例:
# docs/plan/<other-phase>/   # 別フェーズの計画書
# .claude/skills/<other>/    # 他のスキル
```

### 進捗反映先

- 計画書: `{{plan_doc}}` の `§{{phase_id}}` 末尾に「レビュー反映 (YYYY-MM-DD, ラウンド N)」ブロックを追記

### 開始手順

1. 上記必読ドキュメントを読み、`{{plan_doc}}` の `§{{phase_id}}` の現状を把握する
2. `ImplementationLoop.md` の「起動チェック」→「Step 1（並列レビュー）」から開始する
3. 各ラウンドの集約・修正・再レビューは本ファイルの手順に従う
4. **MEDIUM 以上ゼロ** で終了。ループ完了後にユーザーへ最終サマリ（ラウンド毎の件数推移・繰越項目・新規追加テスト）を報告する

---

## ループ自体のメンテナンス

このスキル自体も品質収束する。新フェーズで適用した後、新しい知見が出たら本ファイルの「知見（実績ベース）」セクションに追記する。
