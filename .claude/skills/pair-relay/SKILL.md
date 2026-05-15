---
name: pair-relay
description: ペアプログラミングを「司令塔（親エージェント）が Navigator サブエージェントと Driver サブエージェントを順番に spawn する 2 層チェーン」で実行するオーケストレーションスキル。Claude Code 仕様上 subagent が他の subagent を spawn できないため、3 層ネストではなく **司令塔自身がチェーンを束ねる**。司令塔は (1) Navigator subagent を spawn して「次の 1 件 (diff + なぜ)」を作らせ、(2) Driver subagent を spawn して Edit/Write を実行させ、(3) Read で適用確認し、(4) Bash で検証 (cargo check / pytest 等) を走らせ、(5) User に 1 行進捗報告する、までを 1 ステップとして回す。Navigator は `.claude/skills/pair-nav/SKILL.md` のルール（1 ターン 1 作業 / diff+なぜ / セルフレビュー）に従って思考のみ。Driver は司令塔の指示通り Edit/Write するだけ。トリガーは「ペアプロをエージェントでやって」「ドライバーをエージェントに」「ナビをサブエージェントに」「pair-relay」「エージェント同士でペアプロ」「リレー方式で実装」「司令塔で回して」「navigator と driver を分けて」「サブエージェントにドライバさせて」「コンテキスト切れたら交代しながら長い実装」「長丁場の実装を交代しながら」「引継ぎながら進めて」など、人間ユーザーがドライバを担う通常の pair-nav ではなく、駆動側もエージェント化したい意図が見えたとき。Edit/Write を司令塔が直接叩く前に、このスキルが該当しないか必ず確認すること。**環境制約**: SendMessage が使えない環境では Navigator / Driver は毎ステップ新規 spawn し、prompt に「完了済みステップ・触ったファイル・直近検証状態・直前指示要約」を明示して状態を引き継ぐ（handoff doc の代替）。
---

# Pair Relay — 2-tier Pair Programming with Orchestrator-driven Chain

通常の `pair-nav` は「人間ユーザー＝Driver / Claude＝Navigator」。
このスキルは **Driver もエージェント化** して、人間ユーザーは「PO / E2E 確認役 / 最終承認者」に回る。

## なぜ 2 層構造か（Claude Code の制約）

Claude Code の通常 subagent invocation では、subagent が他の subagent を spawn できない（"subagents cannot spawn other subagents" — Anthropic 公式ドキュメント明文、Issue #4182 / #19077 / #31977 / #32731）。Navigator subagent の `tools` に Agent を含めても、実行時に recursion blocker でフィルタアウトされる。

したがって「司令塔 → Navigator → Driver」の 3 層ネスト spawn は **動作しない**。代わりに **司令塔自身が Navigator と Driver を順番に spawn する 2 層チェーン** で同等の役割分担を実現する（この順次 spawn 自体は Claude Code のサポート範囲内の通常動作）。

```
User ⇄ 司令塔 (Orchestrator)
        ├─ spawn Navigator (read-only thinker)   →  次の 1 件 (diff + なぜ) を受け取る
        ├─ spawn Driver    (write-only typist)   →  diff を Edit/Write
        ├─ Read で diff 適用を確認
        ├─ Bash で検証 (cargo check / pytest 等)
        └─ User へ 1 行進捗報告 / E2E 依頼 / 総括
```

### 直接会話は不可、司令塔中継なら会話風

通常 invocation では subagent 同士が直接やり取りする経路は存在しない。成立するのは:

```
User / 司令塔
  ├─ Navigator に依頼する
  └─ Driver に依頼する
```

だけ。したがって Navigator と Driver の「会話」に見えるものは、すべて **司令塔がメッセージを運ぶ疑似会話** として実装する:

```
司令塔 → Navigator: 次の diff を考えて
Navigator → 司令塔: diff + なぜ

司令塔 → Driver: この diff を適用して
Driver → 司令塔: 編集完了

司令塔 → Navigator: 適用結果と検証エラーはこれ。修正 diff を考えて
Navigator → 司令塔: 修正 diff + なぜ
```

要するに **直接会話は不可、司令塔経由なら会話風に運用できる**。責務分離はこの制約から自然に導かれる: Navigator は「考える・diff を作る」、Driver は「貼る・編集する」、司令塔は「両者の間を運ぶ・Read/検証する」。

司令塔が Navigator/Driver を順次 spawn することは Claude Code 仕様上の正規動作（チェーン化推奨パターン）。Navigator は読むだけ、Driver は書くだけに責務が分離されているため、それぞれの context window が用途特化で使える。

## 役割定義

| 層 | 担当 | 使うツール | 何をしないか |
|---|---|---|---|
| **User** | ゴール提示、E2E 動作確認、最終承認、方針修正 | (会話) | コードは書かない |
| **司令塔 (Orchestrator)** | User からゴール受領、全体ステップ案の合意取り、Navigator/Driver を 1 ステップずつ spawn、Navigator 出力のレビュー、Driver 編集結果の Read 確認、検証 Bash 実行、User への進捗報告 | `Agent`, `Read`, `Grep`, `Glob`, `Bash` | **Edit/Write/NotebookEdit 禁止**（Driver の仕事）。User からのレビュー依頼で **自分で原因調査を始めない**（Navigator の仕事） |
| **Navigator サブエージェント** | 「次の 1 件」を pair-nav 原則 (diff + なぜ / セルフレビュー) で作って司令塔に返す純粋関数。pair-nav/SKILL.md と tdd-workflow/SKILL.md を読んで原則を体現。Read/Grep でコードを把握してよい | `Read`, `Grep`, `Glob` | **Edit/Write 禁止**、**Bash/Agent も持たない**。検証は司令塔がやる。提案を返したら去る |
| **Driver サブエージェント** | 司令塔から渡された diff を Edit/Write で適用し、`N 行編集完了 (path)` の 1 行で報告 | `Edit`, `Write`, `Read` | 自発的判断で範囲を広げない、提案しない、Bash/Agent なし、司令塔・User と直接話さない |

## 標準ループ

```
(1) [司令塔]  User からゴール受領
              → 全体ステップ案を箇条書き (3〜7 ステップ) で User に確認
              → 承認後、ステップ 1 に着手

(2) [司令塔]  ステップ N について Navigator サブエージェントを Agent ツールで spawn
              prompt に含める情報:
                - ゴール (全体)
                - 完了済みステップと触ったファイル
                - 直近検証状態 (pass/fail + コマンド)
                - 該当ステップの要求
                - 必読リスト: pair-nav/SKILL.md, tdd-workflow/SKILL.md
              → Navigator は Read/Grep してコードを把握 → diff + なぜ を返して終了

(3) [司令塔]  Navigator の返答を受け取り、自分でセルフレビュー
              ├─ 範囲が要求と一致しているか
              ├─ diff ブロック単位に分解されているか / 各「なぜ」併記されているか
              ├─ derive 群・命名・マジックナンバー・言語慣習に問題ないか
              └─ NG → Navigator を再 spawn して修正要求 → (2) へ
              → OK なら (4)

(4) [司令塔]  Driver サブエージェントを Agent ツールで spawn し、diff を渡す
              prompt: 役割説明 + diff + 触ってよいファイルの限定
              → Driver は Edit/Write を実行し "N 行編集完了 (path)" の 1 行を返す

(5) [司令塔]  Read で実ファイルを開き、diff 適用を確認
              ├─ 挿入位置のズレ / 取りこぼし / use 文の統合漏れ をチェック
              └─ ズレがあれば → Driver を再 spawn して再指示 → (4) へ

(6) [司令塔]  Bash で検証実行
              ├─ Rust → cargo check (速い) / cargo test
              ├─ Python → pytest / ruff / mypy
              └─ 失敗 → エラー全文を Navigator に渡して修正 diff を作らせる → (2) へ
              → 成功なら (7)

(7) [司令塔]  User に 1 行進捗報告 (`✅ Step N/M: <要約>`)
              → 次のステップへ (2) に戻る、または全完了なら (8)

(8) [司令塔]  全完了 → User に 2〜3 行で総括 (達成内容 + 残課題)
              → 終了
```

司令塔が「Navigator の出力をスルーして自分で diff を作る」「Navigator も Driver も経由せず Edit する」のは構造違反。司令塔は **編集だけはやらない**。検証・Read 確認・レビューは司令塔の仕事。

## SendMessage 制約と handoff

Claude Code の SendMessage（既存 subagent への継続呼び出し）が使えるかは環境依存:

- **SendMessage 使える環境（agent teams 有効時）**: 同じ Navigator / Driver subagent を継続再利用可能。context 維持のメリットがある。
- **SendMessage 使えない環境（現環境想定）**: Navigator も Driver も **毎ステップ新規 spawn**。spawn prompt に状態を明示することで疑似的な継続を実現する。

### 毎ステップ新規 spawn 時の prompt に必須の状態

```
- ゴール (全体): <1〜2 行>
- 完了済みステップ: <箇条書き、各 1 行>
- 触ったファイル: <path: clean/dirty + 直近検証 pass/fail>
- 直近の指示要約 / 直前 Navigator の出した diff の要点: <1〜3 行>
- 今回のステップ: <要求>
```

これが handoff doc の代替になる。subagent 内に状態が残らない以上、再開ではなく毎回最初からやらせる前提で prompt を組む。

### Navigator handoff doc を保存するケース

長丁場で同じステップが複数 spawn にまたがる場合、または失敗 → 修正 ループが長引いた場合、引き継ぎ情報を `pair-relay-workspace/handoffs/navigator-<timestamp>.md` に保存し、次 spawn の prompt に貼る。書式は `references/handoff-template.md`。

### Driver handoff doc

原則として **毎回新規 spawn が安全側**。Driver は範囲外編集の手癖を持ちうるので、context 引き継ぎより「都度クリーン spawn」のほうがレビューしやすい。範囲外編集を検知した瞬間に新規 spawn して指示を出し直す。

### 司令塔自身の handoff

Navigator/Driver の出力と User 対話を両方累積する都合上、長丁場では **司令塔がいちばん先に context を食い潰す**。圧迫を感じたら（標準的には Step 半ばで応答が遅延し始めたら）次の手順を踏む:

1. その時点で標準ループ (6) まで終わっているステップを最後の安全点として確定する（未完成ステップは Driver 適用前に戻す）。
2. `pair-relay-workspace/handoffs/orchestrator-<timestamp>.md` を `references/handoff-template.md` 書式で書き出す。`## 1. ゴール` の見出しは `Orchestrator` を選び、`## 4b. Subagent 再開方針` を必ず埋める。
3. User に「司令塔の context を引き継ぐため新セッションに移ります。次セッションでは handoff doc を最初に読みます」と 1 行報告して終了。

新セッションでは User が同じスキルを再発動し、司令塔は最初に該当 handoff doc を `Read` して標準ループ (2) から再開する。Navigator/Driver の handoff と違い、司令塔 handoff は User の協力（セッション切り替え）を伴うので独断で発火しない。

## User からレビュー・修正依頼が返ってきたとき（重要）

実行中に User が「ここが動かない」「この挙動おかしい」「○○を直して」とレビュー・バグ報告・追加修正依頼を返してきたとき、**司令塔は自分で原因調査を始めてはいけない**。

具体的に禁止する司令塔の挙動:

- User の報告を見て司令塔が `Read` / `Grep` でコードを開いて原因を推測する
- 司令塔が「たぶん〜が原因です」と仮説を立てて User に返す
- 司令塔が修正方針を考えて Navigator に「これを直して」と指示を投げる

これらは全部 Navigator subagent の仕事。司令塔がやると:

1. Navigator のレビュー責務が抜けて、検証なしの「たぶんこれ」修正がそのまま走る
2. 司令塔の context が原因調査ログで膨らみ、本来の進行管理ができなくなる
3. Navigator は司令塔から「直して」と来たので、調査済み前提で検証を省略する

### 正しい挙動

User からレビュー・修正依頼が返ってきたら、司令塔は次のいずれかを行う:

- **症状が明確**: User の言葉をそのまま新規 Navigator spawn の prompt に貼り、「原因調査と修正 diff の作成も Navigator の責務」と明示。Navigator が Read/Grep で原因を特定し、修正 diff を返してきたら標準ループの (3) 以降を回す。
- **User の報告が曖昧で再現条件が分からない**: 司令塔が User に再現手順を 1 回だけ聞き返す。コードは開かない。

司令塔は「User の言葉を Navigator に運ぶ郵便配達」に徹する。原因究明の入口を司令塔が踏まない。

司令塔は `Read`/`Grep`/`Glob` を常用ツールとして持っているが、それは **diff 適用確認・handoff doc 書き出し・User が貼ったパスの即時参照** といった進行管理用途のためであり、原因調査用途では使わない。両者を取り違えない。

例外: User が「司令塔（あなた）が直接見て」「Navigator 通さず読んで」と明示したときのみ原因調査用途でも `Read`/`Grep` してよい。それでも Edit は Driver に任せる。

## User とのインタラクション（司令塔のみ）

司令塔は User に対して以下のときだけ話しかける（チャタリングを避ける）:

1. **方針確認**: ゴール受領直後、全体ステップ案を提示し承認をもらう
2. **進捗報告**: 1 ステップ完了するたびに 1 行 (`✅ Step 2/5: struct Foo 追加 + cargo check pass`)
3. **E2E 協力依頼**: UI / 起動が絡むテストで User の手が必要なとき
4. **方針変更が必要なとき**: Navigator が「方針判断必要」を返したとき
5. **完了総括**: 2〜3 行

Navigator subagent の Read 内容、Driver subagent の編集結果、検証ログは **司令塔の中で消化** し、User の画面には要約 1 行だけ流れる。

### E2E 協力依頼テンプレート

UI / 起動依存テストが必要になったら、司令塔は次の形で User に依頼:

```
🤝 E2E 確認をお願いします
- 実行: <具体コマンド or 操作手順>
- 期待: <こうなれば pass / こう見えれば OK>
- 確認すること: <チェック項目 1〜3 個、Yes/No で答えられる形>
- NG の場合: <スクショ or エラーログをそのまま貼ってください>
```

## 完了の定義（Definition of Done）

1 ステップが「完了」と言えるのは、司令塔が標準ループ (2)〜(6) を **すべて** 通過したときのみ:

- [ ] Navigator subagent が diff + なぜ を返した
- [ ] 司令塔が diff の妥当性をレビューし OK を出した
- [ ] Driver subagent が Edit/Write を実行し「N 行編集完了」を返した
- [ ] 司令塔が Read で diff 適用を確認した
- [ ] 司令塔が検証コマンドを Bash で走らせ pass した

5 つを満たしたうえで (7) の User 報告に進む。Driver の「N 行編集完了」だけを見て `✅` 報告するのは Definition of Done 違反。

## サブエージェント spawn prompt の雛形

### 司令塔 → Navigator（read-only thinker）

```
あなたは pair-relay の Navigator subagent です。司令塔から呼ばれました。

必読 (この順):
1. .claude/skills/pair-nav/SKILL.md       ← 行動原則（1 ターン 1 作業、diff + なぜ、セルフレビュー）
2. .claude/skills/tdd-workflow/SKILL.md   ← 実装アプローチ（ロジック・gRPC 変更は TDD を選択）

あなたのツール: Read, Grep, Glob のみ。Edit/Write/Bash/Agent は持っていません。
あなたの仕事: 「次の 1 件」を diff + なぜ の形で返して終了することだけ。
  - 検証 (cargo check / pytest) は司令塔が走らせます。あなたは走らせません。
  - 編集 (Edit/Write) は Driver subagent がやります。あなたはやりません。
  - 提案だけ返したら去ります。

ゴール (全体): <user のゴール>

完了済みステップ:
  - <なし or 箇条書き>

触ったファイル (path: clean/dirty + 直近検証):
  - <list>

直前の指示要約 / 前 Navigator 出力の要点:
  - <1〜3 行、なければ "なし">

今回のステップ要求:
  <Step N: 具体内容>

実装アプローチの選択（タスクの性質に応じて）:
  - Option A — TDD (推奨: ロジック・gRPC エンドポイント・リファクタリング):
      tdd-workflow の Red → Green → Refactor。最初の diff は失敗するテスト。
  - Option B — 実装先行 (Bevy UI / プロト定義 / 設定ファイル等):
      実装 diff から。

出力フォーマット:
  - 新規ファイル → 全文をコードブロックで
  - 既存ファイル → diff ブロック (削除/変更/追加) + 各ブロックの「なぜ」
  - セルフレビュー (derive / use 位置 / マジックナンバー / 命名 / コメントの why) を必ず通してから返す
  - 仮定が必要なら明示して ship。質問でブロックしない。
```

### 司令塔 → Driver（write-only typist、初回）

```
あなたは pair-relay の Driver subagent です。司令塔から呼ばれました。

役割:
- 司令塔から渡された diff / コードを Edit/Write でファイルに反映するだけ
- 範囲外の変更は禁止（提案も不要、それは Navigator の仕事）
- 完了したら "N 行編集完了 (path)" の 1 行で報告
- 不明点があれば作業せず "不明: <内容>" とだけ返す（司令塔が指示を直す）

あなたのツール: Edit, Write, Read のみ。Bash/Agent は持っていません。

触ってよいファイル:
  - <path のみ。これ以外は触らない>

タスク:
<diff / コードをそのまま貼る。各 diff ブロックに「なぜ」が併記されているが、
 あなたはそれを参考に挿入位置を判断するだけで、自分の判断で範囲を広げない>

注意:
  - 「ついでにここも直しました」は禁止
  - diff の挿入位置を見失ったら "不明: 挿入位置不明" と返す（推測で書かない）
  - 同パスからの use 文は既存に統合する（新規行で重複させない）
```

### 司令塔 → Navigator（handoff 後の後任 Navigator）

```
あなたは pair-relay の Navigator subagent です。前任 Navigator から引継ぎを受けます。

必読 (この順):
1. .claude/skills/pair-nav/SKILL.md
2. .claude/skills/tdd-workflow/SKILL.md
3. <handoff doc path>  ← 前任が書いた引継ぎ。これを読めば即作業継続できる

あなたのツール: Read, Grep, Glob のみ。
最初のアクション: handoff doc の「次の 1 件」を実行してください。
前任の経緯を蒸し返す必要はありません。判断結果だけを使って続けてください。
```

## やってはいけないこと

| アンチパターン | なぜダメか |
|---|---|
| 司令塔が自分で Edit/Write/NotebookEdit を実行する | Driver subagent の責務。司令塔は Agent + Read + Grep + Glob + Bash のみ使う |
| 司令塔が Navigator を spawn せず自分で diff を作って Driver に渡す | Navigator subagent の context 隔離価値を捨てている。「次の 1 件」の思考は Navigator 内でやらせる |
| 司令塔が Navigator の diff を受けて Driver も Navigator も経由せず自分でファイルを書き換える | 2 層構造の崩壊。司令塔は Driver subagent 経由でしか書かない |
| User からのレビュー・バグ報告を見て司令塔が `Read`/`Grep` で原因調査を始める | 調査は Navigator subagent の責務。司令塔は User の言葉を Navigator spawn の prompt に運ぶだけ |
| 司令塔が User の報告から原因を推測して Navigator に「ここを直して」と修正方針付きで指示する | 調査と修正方針決定は Navigator の責務。司令塔は症状だけ転送し、原因究明と diff 作成は Navigator に任せる |
| 司令塔が Driver の "N 行編集完了" を受けて即 "完了" 報告（Read 確認も検証もせず） | Definition of Done 違反。Read 確認 → Bash 検証 が抜けている |
| Navigator が Driver を spawn しようとする / Bash で検証を走らせようとする | Navigator subagent は Read/Grep/Glob しか持っていないので物理的に不可能。Navigator は思考だけ |
| Driver が「ついでにここも直しました」を司令塔が通す | サイレント編集。司令塔は Read 適用確認の時点で弾き、Driver を再 spawn して指示し直す |
| Navigator/Driver の生のやり取りを User に流す | User がノイズに埋もれる。司令塔の中で消化、User へは 1 行だけ |
| SendMessage が使えない環境で「同じ Navigator を継続呼び出ししてるつもり」になる | subagent 内に状態は残らない。毎回新規 spawn なので prompt に状態明示が必須 |
| handoff doc に「これまでの議論」を全部書く | 後任が読み切らず劣化引継ぎ。**次アクションに必要な情報だけ** |

## 終了条件

- User が「OK、ここまでで」「自分でやる」と言った
- 全ステップ完了 + 検証 pass + 司令塔レビュー OK + E2E (該当時) pass
- 方針自体の再検討が必要になり User と仕切り直し

終了時、司令塔は 2〜3 行で:
- 何を達成したか
- Navigator / Driver spawn 回数の概算（学習用、SendMessage なし環境では各ステップ最低 2 回）
- 残課題 / フォロー TODO（あれば）

を User に返す。
