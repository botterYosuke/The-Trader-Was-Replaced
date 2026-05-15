---
name: pair-relay
description: ペアプログラミングを「司令塔（親エージェント）→ Navigator サブエージェント → Driver サブエージェント」の**単方向チェーン**で実行するオーケストレーションスキル。司令塔は Navigator を spawn したら結果待ちに徹し、Navigator が `.claude/skills/pair-nav/SKILL.md` のルール（1ターン1作業 / diff+なぜ / セルフレビュー / Read 再確認 / 検証実行）に従って **Driver サブエージェントを自ら spawn し、Edit/Write 結果をレビューし、cargo check 等の検証を Bash で走らせる**。Driver は Navigator の指示で Edit/Write するだけ。司令塔は Navigator から返ってくる完了報告を User に中継し、長丁場のときは Navigator を別エージェントに handoff で差し替える。トリガーは「ペアプロをエージェントでやって」「ドライバーをエージェントに」「ナビをサブエージェントに」「pair-relay」「エージェント同士でペアプロ」「リレー方式で実装」「司令塔で回して」「navigator と driver を分けて」「サブエージェントにドライバさせて」「コンテキスト切れたら交代しながら長い実装」「長丁場の実装を交代しながら」「引継ぎながら進めて」など、人間ユーザーがドライバを担う通常の pair-nav ではなく、駆動側もエージェント化したい意図が見えたとき。Edit/Write を司令塔が直接叩く前に、このスキルが該当しないか必ず確認すること。
---

# Pair Relay — 3-tier Pair Programming with Agent Handoff

通常の `pair-nav` は「人間ユーザー＝Driver / Claude＝Navigator」。
このスキルは **Driver もエージェント化** して、人間ユーザーは「PO / E2E 確認役 / 最終承認者」に回る。

**重要な構造**: 司令塔と Driver は**直接対話しない**。チェーンは一方向で:

```
User ⇄ 司令塔 ⇄ Navigator ⇄ Driver
              ↑           ↑
              spawn       spawn
```

司令塔は Navigator を spawn したらあとは結果待ち。Navigator が自分で Driver を spawn し、Driver の出力をレビューし、検証 (cargo check / pytest 等) を Bash で走らせ、完了したステップを司令塔に報告する。司令塔が Navigator と Driver の間に入って中継してはいけない（それをやると Navigator のレビュー責務が司令塔に染み出して曖昧になる）。

## 役割定義

| 層 | 担当 | 使うツール | 何をしないか |
|---|---|---|---|
| **User** | ゴール提示、E2E 動作確認、最終承認、方針修正 | (会話) | コードは書かない |
| **司令塔 (Orchestrator)** | User からゴール受領、全体ステップ案の合意取り、Navigator の spawn、Navigator の context 監視 → handoff swap 発火、User への進捗報告と協力依頼 | `Agent`, `Read`, `Grep`, `Glob` | **Edit/Write/NotebookEdit 禁止**（Driver の仕事）。**検証コマンド (cargo check / pytest 等) も走らせない**（Navigator の仕事）。**Driver と直接対話しない**（Navigator の仕事を奪う） |
| **Navigator サブエージェント** | 「次の 1 件」を pair-nav のルールで diff+なぜ で作る、**Driver を Agent ツールで spawn して指示を渡す**、Driver の編集結果を Read で確認、`cargo check`/`pytest` 等を Bash で実行、コードレビュー、ステップ完了を司令塔に報告、自身の context が厳しくなったら handoff doc を書いて司令塔に返す | `Agent`, `Bash`, `Read`, `Grep`, `Glob` | **Edit/Write 禁止**（pair-nav 原則 1）。範囲外の変更を Driver に指示しない |
| **Driver サブエージェント** | Navigator の指示通りに Edit/Write を実行し、結果を 1〜2 行で Navigator に報告 | `Edit`, `Write`, `Read` | 自発的判断で範囲を広げない、提案しない、司令塔・User と直接話さない |

司令塔が編集や検証を肩代わりすると Navigator の存在意義が消える。**例外**: Navigator が 2 連続で同じ問題に詰まり handoff doc も書けない状態のとき、司令塔は介入を User に明示してから直接 Read/Grep で状況を把握する。それでも Edit は Driver（または新規 Navigator 経由で）に任せる。

## 標準ループ

```
(1) [Orchestrator]  User からゴール受領
                    → 全体ステップ案を箇条書き (3〜7 ステップ) で User に確認
                    → 承認後、「Navigator spawn prompt 雛形」で
                      Agent ツールで Navigator サブエージェントを spawn

(2) [Orchestrator]  Navigator の完了報告を待つ
                    （途中経過は流れてこない。Navigator が次のいずれかで返ってくる）
                    
                    A) ステップ群を消化して "全部完了"        → (5)
                    B) "次の 1 ステップ完了" の細切れ報告     → (3)
                    C) context が厳しくなった ＋ handoff doc → (4)
                    D) User 判断が必要 (方針再考)            → User に確認 → (1) へ

(3) [Orchestrator]  細切れ完了報告 (B) を受けた場合
                    → User に 1 行報告 (`✅ Step 2/5: ...`)
                    → Navigator の連続ターン数を加算
                    → 閾値未満なら同じ Navigator に「次のステップへ」と返答
                    → 閾値超過なら (4) の handoff を発火

(4) [Orchestrator]  Navigator handoff
                    → 現 Navigator に「handoff doc を書いて」と最後の指示
                    → 受け取った handoff doc を保存
                    → 新 Navigator を spawn (handoff doc + pair-nav/SKILL.md を読ませる)
                    → (2) へ

(5) [Orchestrator]  全ステップ完了報告を受けたら
                    → User に 2〜3 行で総括 (達成内容 + swap 回数 + 残課題)
                    → 終了
```

司令塔が「Navigator の出力を見て Driver に貼り直す」「Navigator 経由せず Driver を直接呼ぶ」「Navigator 経由せず自分で cargo check を回す」のはすべて構造違反。チェーンは一方向。

## Navigator の内部ループ（Navigator サブエージェントが従う）

Navigator は spawn 直後に **必ず `.claude/skills/pair-nav/SKILL.md` を読む**。pair-nav の原則 (1〜5) に加えて、pair-relay 固有の責務として **Driver の管理と検証実行** を担う。

```
[Navigator 内ループ]

(N1) 次の 1 件を作る
      ├─ 新規ファイル → 全文
      ├─ 既存ファイル → diff ブロック + 各「なぜ」
      ├─ セルフレビュー (derive / use 位置 / マジックナンバー / 命名 / コメントの why)
      ├─ 仮定が必要なら明示して ship（質問でブロックしない）
      └─ 実装アプローチの選択（タスクの性質に応じて）:
           Option A — TDD (推奨: ロジック・gRPC エンドポイント・リファクタリング):
               .claude/skills/tdd-workflow/SKILL.md の Red→Green→Refactor サイクルに従う
               (N1a) テストを先に書く diff を Driver に渡す → (N2)
               (N1b) 検証 (uv run pytest) で FAILED を確認 → (N4)
               (N1c) 最小実装 diff を Driver に渡す → (N2)
               (N1d) 検証で GREEN を確認 → リファクタリングへ
           Option B — 実装先行 (Bevy UI / プロト定義 / 設定ファイル等):
               diff ブロック + なぜ → Driver → Read 確認 → cargo check/pytest → レビュー

(N2) Driver を Agent ツールで spawn
      ├─ 初回 → 「Driver spawn prompt 雛形（初回）」
      └─ 2 回目以降の同タスク → 「Driver spawn prompt 雛形（継続）」
                              （ファイル context や直前の指示を引き継ぐ）

(N3) Driver の "N 行編集完了 (path)" を受け取ったら、
      まず Read でファイルを開いて diff 適用を確認
      （pair-nav 原則: 検証ツールを回す前に Read で確認）
      ├─ 挿入位置のズレ / 取りこぼし / use 文の統合漏れ をチェック
      └─ ズレがあれば → Driver に再指示 → (N2) へ

(N4) 検証コマンドを Bash で実行
      ├─ Rust → cargo check (速い) / cargo test
      ├─ Python → pytest / ruff / mypy
      ├─ TypeScript → tsc --noEmit / npm test
      └─ 失敗 → エラーを読み、修正 diff を作って Driver に渡す → (N2) へ

(N5) コードレビュー（コンパイル通過とは別軸）
      ├─ derive 群 / 命名 / マジックナンバー / 言語慣習
      ├─ 範囲外の変更 (Driver の「ついで」) が混ざっていないか
      └─ NG → 修正 diff を Driver に → (N2) へ

(N6) ステップ完了。司令塔に 1 行報告
      ├─ 通常: 「Step N 完了: <要約 1 行>。続行可」
      ├─ context 厳しい: 「Step N 完了。handoff 推奨。以下 handoff doc:」
      ├─ 方針判断必要: 「Step N の途中で X の判断が必要。User に確認求む」
      └─ ステップ群全部終わり: 「全完了。検証 pass。総括: ...」
```

### Navigator が司令塔に話しかける唯一のタイミング

Navigator は内ループの (N6) 以外で司令塔に話しかけない。Driver とのやりとり、検証ログ、レビュー差し戻しは **Navigator の中で閉じる**。司令塔の context を消費しない。

### Navigator が Driver を再 spawn するタイミング

- **毎ステップ毎に新 Driver**: 不要。Driver は同じファイル群を触り続けるので、context を引き継いだほうが速い。原則として **1 Navigator セッションで 1 Driver** を継続使用する。
- **Driver の context が満ちてきたら**: Navigator が判断する目安は「同一 Driver への連続指示 20 回超」「Driver が同じ指示を 2 回間違える」「Driver 自身が context 警告を返した」のいずれか。発生したら:
  1. 現 Driver に「これからあなたを別の Driver に交代します。後任が読めば即作業継続できる handoff doc を `references/handoff-template.md` の構造で書いてください」と指示
  2. handoff doc を `pair-relay-workspace/handoffs/driver-<timestamp>.md` に保存
  3. 新 Driver を Agent ツールで spawn し、handoff doc を最初に読ませる
- **Driver が暴走した（範囲外編集など）**: 即座に新 Driver に交代。前任の手癖を引き継がせない。

## Handoff Swap — Navigator の交代

### いつ swap するか（司令塔の判定 heuristics）

司令塔は Navigator の **連続ターン数** と **応答内容** を追跡し、以下で swap を発火:

| シグナル | 閾値 (目安) |
|---|---|
| 同一 Navigator への連続ターン数 (= 完了報告の回数) | **8 ターン超** |
| Navigator が「context が厳しい」「これまでの経緯が曖昧」と訴える | 1 回でも出たら即時 |
| Navigator が同じ提案を 2 回繰り返す / 既に決定した方針を蒸し返す | 検出時すぐ |
| `Agent` ツールが返した結果が極端に遅い / トークン消費が急増 | 1 回でも出たら次の境界で |

**境界で swap する**: ステップ中盤ではなく、ある 1 件が完了して次の 1 件に移る瞬間。中途半端な状態を引き継がせない。

Driver-level の swap は **Navigator の内部で完結**するので、司令塔は関知しない。司令塔が監視するのは Navigator のみ。

### handoff doc 生成

swap 直前、司令塔は退役する Navigator に **handoff doc を書かせる**:

> 「これからあなたを別の Navigator に交代します。後任が **これだけ読めば即座に同じ品質で続けられる** 引継ぎ文章を `references/handoff-template.md` の構造で書いてください。冗長な経緯ではなく、後任の次アクションに必要な情報だけ。現在の Driver サブエージェントの状態 (生きているか、context 余裕度) も明記してください。」

司令塔は受け取った handoff doc を `pair-relay-workspace/handoffs/navigator-<timestamp>.md` に保存し、新 Navigator を spawn する prompt にそのまま貼る。

### handoff doc の最小要素

`references/handoff-template.md` 参照。要素だけ列挙:

1. **ゴール（全体）** — 1〜2 行
2. **現フェーズ / 完了済みステップ** — 箇条書き 3〜7 項目、各 1 行
3. **次の 1 件** — 後任が真っ先にやる作業（次に Driver へ出す diff、または次に判断すべきポイント）
4. **触ったファイルと現状** — path + dirty/clean + 直近の検証コマンド結果（pass/fail）
5. **Driver の状態** — 現 Driver は引き継ぐか / 殺すか、引き継ぐなら handoff doc のパス
6. **既知の落とし穴 / User からの制約** — 1〜3 行
7. **やらないこと（明示的に範囲外）** — 1〜2 行
8. **pair-nav 原則のうち、この場面で特に重要なもの** — 1〜2 行

「これまでの会話の議事録」は **書かない**。後任は議事録を読まずに即着手できるよう、判断結果だけを残す。

## User とのインタラクション（司令塔のみ）

司令塔は User に対して以下のときだけ話しかける（チャタリングを避ける）:

1. **方針確認**: ゴール受領直後、全体ステップ案を提示し承認をもらう
2. **進捗報告**: Navigator から細切れ完了報告 (B) が来るたび 1 行 (`✅ Step 2/5: struct Foo 追加 + 検証 pass`)
3. **E2E 協力依頼**: UI / 起動が絡むテストで User の手が必要なとき。Navigator が完了報告に「E2E 確認要」と書いてきたら司令塔が中継する
4. **方針変更が必要なとき**: Navigator が "User 判断必要 (D)" で返したとき
5. **swap 発生報告**: 1 行で `↻ Navigator swap (8 ターン到達) — handoff: <path>`
6. **完了総括**: 2〜3 行

Navigator と Driver の間のやりとり、検証ログ、レビュー差し戻しは **司令塔まで上がってこない** ので、User の画面にも当然流れない。

### E2E 協力依頼テンプレート

UI / 起動依存テストで Navigator が「E2E 必要」と返してきたら、司令塔は次の形で User に依頼:

```
🤝 E2E 確認をお願いします
- 実行: <具体コマンド or 操作手順>
- 期待: <こうなれば pass / こう見えれば OK>
- 確認すること: <チェック項目 1〜3 個、Yes/No で答えられる形>
- NG の場合: <スクショ or エラーログをそのまま貼ってください>
```

## 完了の定義（Definition of Done）

1 ステップが「完了」と言えるのは、Navigator の内ループ (N1)〜(N5) を **すべて** 通過したときのみ:

- [ ] Navigator が diff + なぜ を作った
- [ ] Driver が Edit/Write を実行し「N 行編集完了」を返した
- [ ] Navigator が Read で diff 適用を確認した
- [ ] Navigator が検証コマンドを Bash で走らせ pass した
- [ ] Navigator が実コードをレビューし OK を出した

Navigator がこの 5 つを満たしたうえで (N6) の完了報告を司令塔に返したときだけ、司令塔は User に `✅` 報告できる。Navigator が「Driver が編集しました」だけ報告してきた場合は **未完了** として Navigator に差し戻す（Navigator のレビューと検証を省略させない）。

## サブエージェント spawn prompt の雛形

### 司令塔が Navigator を最初に spawn するとき

```
あなたは pair-relay の Navigator です。司令塔から呼ばれました。

必読 (この順):
1. .claude/skills/pair-relay/SKILL.md  ← 自分の責務（Driver spawn / 検証 / レビュー / 完了報告）
2. .claude/skills/pair-nav/SKILL.md    ← 行動原則（Edit/Write しない、1 ターン 1 作業、diff + なぜ、Read 再確認）
3. .claude/skills/tdd-workflow/SKILL.md ← 実装アプローチ（ロジック・gRPC 変更は TDD を選択する）

ゴール (全体): <user のゴール>
全体ステップ案 (User 承認済み):
  1. <step 1>
  2. <step 2>
  ...

現状:
  完了済みステップ: <なし or 箇条書き>
  触ったファイル: <list>
  直近の検証状態: <pass/fail + コマンド>

あなたの仕事:
  - Step 1 から順に着手
  - 各ステップで: diff 作成 → Driver を Agent ツールで spawn → 結果を Read → 検証 Bash → レビュー
  - 1 ステップ完了するたびに、司令塔に 1 行で完了報告して返答を待つ
    （例: "Step 1 完了: WindowRoot に PanelKind タグ追加。cargo check pass。続行可"）
  - 自分の context が厳しくなったら、handoff doc を書いて司令塔に返す
  - 方針判断が要るところに来たら、司令塔に判断を仰ぐ（自分で勝手に決めない）

Driver の管理:
  - 原則 1 Navigator セッション = 1 Driver。context を引き継いで使い回す
  - Driver が暴走 / 連続 20 回 / context 警告 のいずれかで、Driver を交代させる
  - Driver-level の handoff は Navigator の責任。司令塔には報告不要

最初のアクション: Step 1 に着手してください。
```

### Navigator が Driver を spawn するとき（初回）

```
あなたは pair-relay の Driver です。Navigator から呼ばれました。

役割:
- 私 (Navigator) から渡された diff / コードを Edit/Write でファイルに反映するだけ
- 範囲外の変更は禁止（提案も不要、それは Navigator の仕事）
- 完了したら "N 行編集完了 (path)" の 1 行で報告
- 不明点があれば作業せず "不明: <内容>" とだけ返す（私が指示を直す）

タスク:
<diff / コードをそのまま貼る>

注意: 触っていいファイルは上記タスクに書かれたものだけ。「ついでにここも直しました」は禁止。
```

### Navigator が Driver を spawn するとき（継続: 同一セッション 2 回目以降）

```
Driver さん、続きのタスクです。

前回までで触ったファイル: <list>
前回までの編集状態: clean (検証 pass 済み)

今回のタスク:
<diff / コードをそのまま貼る>

完了したら "N 行編集完了 (path)" で報告してください。
```

### Navigator が Driver を handoff swap するとき

```
Driver さん、ここまでありがとう。あなたを別の Driver に交代します。

後任が **これだけ読めば即作業継続できる** 引継ぎ文章を、以下の構造で書いてください:
1. 触ったファイル一覧 (path + clean/dirty)
2. 直前のタスクと結果
3. 次に来るタスクの傾向（私が次に渡しそうな diff の種類）
4. ハマりやすかったポイント (もしあれば 1〜2 行)

冗長な議事録は不要。後任の次アクションに必要なことだけ。
```

### 司令塔が Navigator を handoff swap するとき

```
Navigator さん、context が厳しくなったので別の Navigator に交代します。

後任が **これだけ読めば即座に同じ品質で続けられる** 引継ぎ文章を、
references/handoff-template.md の構造で書いてください。

特に明記してほしいこと:
- 現 Driver サブエージェントは生きているか
- 生きているなら、新 Navigator が引き継いでよいか（Driver 側の handoff doc のパス）
- 殺すなら、新 Navigator は新規 Driver を spawn することになる
```

### handoff 後の後任 Navigator spawn

```
あなたは pair-relay の Navigator です。前任から引継ぎを受けました。

必読 (この順):
1. .claude/skills/pair-relay/SKILL.md
2. .claude/skills/pair-nav/SKILL.md
3. .claude/skills/tdd-workflow/SKILL.md
4. <handoff doc path>

最初のアクション: handoff doc の「次の 1 件」を実行してください。
前任の経緯を蒸し返す必要はありません。

Driver について:
  - handoff doc に「現 Driver 引継ぎ可」とあれば、Agent ツールで同じ Driver を再呼び出し可能
    （Driver 用 handoff doc を最初に読ませてから新タスクを渡す）
  - 「Driver 殺した」とあれば、新規 Driver を spawn し直す
```

## やってはいけないこと

| アンチパターン | なぜダメか |
|---|---|
| 司令塔が Driver を直接 spawn する | チェーンが `司令塔 → Driver` の二段になり、Navigator のレビュー責務が抜ける。**Driver を spawn するのは Navigator だけ** |
| 司令塔が Navigator の出力（diff）を受け取って自分で Driver に貼り直す | 同上。司令塔が中継した瞬間、構造が壊れる。Navigator が完了報告を返すまで司令塔は待つだけ |
| 司令塔が `cargo check` / `pytest` を自分で走らせる | 検証は Navigator の責務（pair-nav 原則）。司令塔が走らせると Navigator がレビュー前提で動かなくなる |
| Navigator が「diff 作りました」で司令塔に返し、Driver を spawn しない | Navigator の責務放棄。Navigator は Driver spawn → 検証 → レビュー まで完走して初めて 1 ステップ |
| Navigator が Driver の "N 行編集完了" を受けて即 "完了" 報告（検証もレビューもせず） | Definition of Done 違反。Read 確認 → 検証 → レビュー が抜けている |
| 司令塔が Navigator と Driver の生のやり取りを User に流す | User がノイズに埋もれる。Navigator の中で閉じる、司令塔は要約だけ |
| Driver が「ついでにここも直しました」を Navigator が通す | サイレント編集。Navigator はレビューで弾き、Driver に範囲外編集の取り消しを指示する |
| Navigator の context 警告を無視して同じ Navigator に 15 ターン走らせる | 終盤に精度が落ちて手戻り。閾値 (8 ターン目安) で swap |
| handoff doc に「これまでの議論」を全部書く | 後任が読み切らず劣化引継ぎ。**次アクションに必要な情報だけ** |
| swap 後に新 Navigator に `pair-nav/SKILL.md` を読ませ忘れる | 行動原則がリセットされ、勝手に Edit 始める/全文置換始める |

## 終了条件

- User が「OK、ここまでで」「自分でやる」と言った
- 全ステップ完了 + 検証 pass + Navigator レビュー OK + E2E (該当時) pass
- 方針自体の再検討が必要になり User と仕切り直し

終了時、司令塔は 2〜3 行で:
- 何を達成したか
- Navigator swap が何回発生したか（学習用）
- 残課題 / フォロー TODO（あれば）

を User に返す。
