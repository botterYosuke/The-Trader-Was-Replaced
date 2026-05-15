---
name: pair-relay
description: ペアプログラミングを「司令塔（親エージェント）が Navigator サブエージェントと Driver サブエージェントを順番に spawn する 2 層チェーン」で実行するオーケストレーションスキル。Claude Code 仕様上 subagent が他の subagent を spawn できないため、3 層ネストではなく **司令塔自身がチェーンを束ねる**。司令塔は (1) Navigator subagent を spawn して「次の 1 件 (diff + なぜ + セルフレビュー)」を作らせ、(2) Driver subagent を spawn して Edit/Write を実行させ、(3) Read で適用確認し、(4) 検証 Navigator を再 spawn して read-only Bash 検証 (cargo check / pytest 等) を走らせ pass 結果 or 修正 diff を受け取り、(5) User に 1 行進捗報告する、までを 1 ステップとして回す。Navigator は `.claude/skills/pair-nav/SKILL.md` のルール（1 ターン 1 作業 / diff+なぜ / セルフレビュー）に従い、思考と read-only 検証 Bash (cargo check/test, pytest, ruff, mypy) の両方を担う。Driver は司令塔の指示通り Edit/Write するだけ。司令塔は diff の中身レビュー (derive / 命名 / マジックナンバー等) を**しない** — Navigator のセルフレビューチェックリスト全項目通過のみを確認する。トリガーは「ペアプロをエージェントでやって」「ドライバーをエージェントに」「ナビをサブエージェントに」「pair-relay」「エージェント同士でペアプロ」「リレー方式で実装」「司令塔で回して」「navigator と driver を分けて」「サブエージェントにドライバさせて」「コンテキスト切れたら交代しながら長い実装」「長丁場の実装を交代しながら」「引継ぎながら進めて」など、人間ユーザーがドライバを担う通常の pair-nav ではなく、駆動側もエージェント化したい意図が見えたとき。Edit/Write を司令塔が直接叩く前に、このスキルが該当しないか必ず確認すること。**環境制約**: SendMessage が使えない環境では Navigator / Driver は毎ステップ新規 spawn し、prompt に「完了済みステップ・触ったファイル・直近検証状態・直前指示要約」を明示して状態を引き継ぐ（handoff doc の代替）。
---

# Pair Relay — 2-tier Pair Programming with Orchestrator-driven Chain

通常の `pair-nav` は「人間ユーザー＝Driver / Claude＝Navigator」。
このスキルは **Driver もエージェント化** して、人間ユーザーは「PO / E2E 確認役 / 最終承認者」に回る。

## なぜ 2 層構造か（Claude Code の制約）

Claude Code の通常 subagent invocation では、subagent が他の subagent を spawn できない（"subagents cannot spawn other subagents" — Anthropic 公式ドキュメント明文、Issue #4182 / #19077 / #31977 / #32731）。Navigator subagent の `tools` に Agent を含めても、実行時に recursion blocker でフィルタアウトされる。

したがって「司令塔 → Navigator → Driver」の 3 層ネスト spawn は **動作しない**。代わりに **司令塔自身が Navigator と Driver を順番に spawn する 2 層チェーン** で同等の役割分担を実現する（この順次 spawn 自体は Claude Code のサポート範囲内の通常動作）。

```
User ⇄ 司令塔 (Orchestrator)
        ├─ spawn Navigator (diff 作成モード, read-only)   →  次の 1 件 (diff + なぜ + セルフレビュー + 自己検証) を受け取る
        ├─ 契約遵守チェック (チェックリスト全項目通過か / 範囲外編集なしか)
        ├─ spawn Driver    (write-only typist)            →  diff を Edit/Write
        ├─ Read で diff 適用を確認
        ├─ spawn Navigator (検証モード, read-only Bash)   →  cargo check / pytest 等を Navigator 側で実行し
        │                                                    pass 1 行 or fail 要約 + 修正 diff を受け取る
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
司令塔 → Navigator (diff 作成モード): 次の diff を考えて
Navigator → 司令塔: diff + なぜ + セルフレビューチェックリスト + 自己検証ステータス

司令塔 → Driver: この diff を適用して
Driver → 司令塔: 編集完了

司令塔 → Navigator (検証モード): 適用済み diff の要点はこれ。read-only Bash で検証して
Navigator → 司令塔: ✅ pass の 1 行 / または fail 要約 + 修正 diff + なぜ + セルフレビュー
```

要するに **直接会話は不可、司令塔経由なら会話風に運用できる**。責務分離はこの制約から自然に導かれる: Navigator は「考える・diff を作る・read-only Bash で自己検証する」、Driver は「貼る・編集する」、司令塔は「両者の間を運ぶ・Read で適用確認する・進行管理する」。

司令塔が Navigator/Driver を順次 spawn することは Claude Code 仕様上の正規動作（チェーン化推奨パターン）。Navigator は読む＋検証するだけ、Driver は書くだけに責務が分離されているため、それぞれの context window が用途特化で使え、コードレビューと検証ログという「重い読書」を司令塔から外せる。

## 役割定義

| 層 | 担当 | 使うツール | 何をしないか |
|---|---|---|---|
| **User** | ゴール提示、E2E 動作確認、最終承認、方針修正 | (会話) | コードは書かない |
| **司令塔 (Orchestrator)** | User からゴール受領、全体ステップ案の合意取り、Navigator/Driver を 1 ステップずつ spawn、Navigator 出力の**チェックリスト確認**（セルフレビュー全項目が通っているか）、Driver 編集結果の Read 確認、検証 Navigator からの結果受領、User への進捗報告 | `Agent`, `Read`, `Grep`, `Glob`, `Bash` | **Edit/Write/NotebookEdit 禁止**（Driver の仕事）。**diff の中身レビュー禁止**（derive / 命名 / マジックナンバー / 言語慣習等は Navigator のセルフレビュー責務 — 司令塔が再判定すると二重化して context が膨らむ）。**検証 Bash の一次実行を禁止**（検証モード Navigator の責務 — 司令塔が走らせると検証ログが司令塔に蓄積し長丁場で先に枯渇する。司令塔の Bash は git status 等の進行管理用途に限定）。User からのレビュー依頼で **自分で原因調査を始めない**（Navigator の仕事） |
| **Navigator サブエージェント** | (a) **diff 作成モード**: 「次の 1 件」を pair-nav 原則 (diff + なぜ + セルフレビュー全項目通過) で作って司令塔に返す。(b) **検証モード** (Driver 適用後の再 spawn): read-only Bash で検証を実行し、pass なら結果を、fail ならエラー要約 + 修正 diff (なぜ + セルフレビュー併記) を返す。pair-nav/SKILL.md と tdd-workflow/SKILL.md を読んで原則を体現 | `Read`, `Grep`, `Glob`, `Bash` (ソース非変更の検証コマンドのみ: cargo check / cargo test / pytest / ruff / mypy) | **Edit/Write 禁止**、**Agent も持たない**。Bash は副作用のある操作（git commit, rm, ファイル書き換え, ネットワーク, サーバ起動）に絶対使わない。提案 or 検証結果を返したら去る |
| **Driver サブエージェント** | 司令塔から渡された diff を Edit/Write で適用し、`編集完了 (path)` の 1 行で報告 | `Edit`, `Write`, `Read` | 自発的判断で範囲を広げない、提案しない、Bash/Agent なし、司令塔・User と直接話さない |

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
                - **実装アプローチの選択ブロック (Option A: TDD / Option B: 実装先行)**: 司令塔が事前にどちらか決め打ちしてよいが、雛形のこのブロック自体を丸ごと削除して spawn してはいけない。Navigator に判断材料を渡さないと TDD 適切ケースで実装先行に流される
              → Navigator は Read/Grep してコードを把握、必要なら read-only Bash で軽い自己検証
                  → diff + なぜ + セルフレビューチェックリスト全項目 (各項目 [pass]/[n/a] 判定 + 1 行根拠) + 自己検証ステータス を返して終了

(3) [司令塔]  Navigator の返答を受け取り、**契約遵守チェック**のみ実施
              （diff の中身レビュー — derive / 命名 / マジックナンバー等 — は Navigator のセルフレビュー責務。
                司令塔が再判定するとレビュー思考が context に積もり、Navigator の自走圧も下がる）
              ├─ セルフレビューチェックリスト全項目に [pass] or [n/a] 判定 + 1 行根拠が付いているか
              │   （[ ] 未判定のままや、根拠なし [pass] は不備として弾く）
              ├─ 自己検証ステータス (実行コマンド or skip 理由 / 結果) が記載されているか
              ├─ diff がブロック単位に分解され、各「なぜ」が併記されているか
              ├─ 範囲外編集の提案（触ってよいファイル外への変更）が紛れていないか
              └─ NG → 「セルフレビュー不備: 〜が欠落」とだけ伝えて Navigator を再 spawn → (2) へ
                  （中身の品質指摘は司令塔がやらない。やり直しは Navigator に任せる）
              → OK なら (4)

(4) [司令塔]  Driver サブエージェントを Agent ツールで spawn し、diff を渡す
              prompt: 役割説明 + diff + 触ってよいファイルの限定
              → Driver は Edit/Write を実行し "編集完了 (path)" の 1 行を返す

(5) [司令塔]  Read で実ファイルを開き、**Navigator が提示した diff どおりに適用されているか**だけを確認
              （use 統合の正しさ・命名・derive など中身品質の判断は Navigator のセルフレビュー責務。
                ここで司令塔がやるのは「diff に書いてある行が、書いてある位置に入っているか」の機械的照合）
              ├─ 挿入位置のズレ / 取りこぼし / Driver が範囲外を触っていないか をチェック
              └─ ズレがあれば → Driver を再 spawn して再指示 → (4) へ

(6) [司令塔]  **検証 Navigator** を spawn して read-only Bash で検証実行させる
              （司令塔自身は走らせない。Bash 実行ログが司令塔に蓄積すると長丁場で先に context が尽きる）
              prompt に含める情報:
                - 役割: 検証モード Navigator (diff 作成ではなく、Driver 適用済みコードの検証)
                - 触ったファイル / 適用済み diff の要点
                - 推奨検証コマンド: Rust → cargo check / cargo test, Python → pytest / ruff / mypy
                - 返す形式: pass の場合 → "✅ <コマンド>: pass" の 1〜2 行のみ
                              fail の場合 → エラー**要約** (5〜15 行に圧縮) + 修正 diff + なぜ + セルフレビュー
              → fail の修正 diff が返ってきたら (3) へ（契約遵守チェック → Driver → ...）
              → pass なら (7)
              ※ 司令塔はエラーログ全文を保持しない。Navigator の要約だけを受け取り、詳細が必要なら
                 後続 Navigator spawn 時に「前ターン要約のみ」を渡す。Navigator が自前 Bash で
                 詳細を再取得する。

(7) [司令塔]  User に 1 行進捗報告 (`✅ Step N/M: <要約>`)
              → 次のステップへ (2) に戻る、または全完了なら (8)

(8) [司令塔]  全完了 → User に 2〜3 行で総括 (達成内容 + 残課題)
              → 終了
```

司令塔が「Navigator の出力をスルーして自分で diff を作る」「Navigator も Driver も経由せず Edit する」のは構造違反。司令塔は **編集も検証 Bash の一次実行も diff 中身レビューもやらない**。司令塔の仕事は: 進行管理・spawn 統括・Read による diff 適用確認・Navigator 出力の契約遵守チェック・User 対話。検証コマンドの実行と diff の中身判断は Navigator の仕事。

## SendMessage 制約と handoff

Claude Code の SendMessage（既存 subagent への継続呼び出し）が使えるかは環境依存:

- **SendMessage 使える環境（agent teams 有効時）**: 同じ Navigator / Driver subagent を継続再利用可能。context 維持のメリットがある。ただし **同一モード内に限る** — diff 作成モード Navigator を検証モードに転用してはいけない（独立検証の前提が崩れ、自分で書いた diff を自分で検証する自己採点状態になる）。検証モード Navigator は常に新規 spawn（または検証モード専用の SendMessage チャネル）として diff 作成モードと分離する。Driver も diff 適用ごとに新規 spawn が安全側（[Driver handoff doc 節](#driver-handoff-doc) 参照）。
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

Navigator/Driver の出力と User 対話を両方累積する都合上、長丁場では **司令塔がいちばん先に context を食い潰す**（diff 中身レビューと検証 Bash の一次実行を Navigator に寄せた後でも、累積する性質自体は変わらない）。圧迫を感じたら（標準的には Step 半ばで応答が遅延し始めたら）次の手順を踏む:

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

Navigator subagent の Read 内容、Driver subagent の編集結果、**検証 Navigator が返した要約（フルログではない）** は司令塔の中で消化し、User の画面には要約 1 行だけ流れる。検証ログ全文は検証 Navigator の context に閉じ、司令塔には届かない（これが今回の負荷軽減の主目的）。

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

- [ ] Navigator subagent (diff 作成モード) が diff + なぜ + セルフレビューチェックリスト + 自己検証ステータス を返した
- [ ] 司令塔が**契約遵守チェック**で OK を出した（チェックリスト全項目 + 各項目の根拠 + 範囲外編集なし）
- [ ] Driver subagent が Edit/Write を実行し「編集完了 (path)」を返した
- [ ] 司令塔が Read で diff 適用を確認した
- [ ] 検証 Navigator (検証モード) が read-only Bash で検証し pass を返した

5 つを満たしたうえで (7) の User 報告に進む。Driver の「編集完了」だけを見て `✅` 報告するのは Definition of Done 違反（Read 確認と検証 Navigator が抜けている）。司令塔が自分で diff レビューや Bash 検証を肩代わりするのも違反（責務逸脱で context が偏る）。

## サブエージェント spawn prompt の雛形

### 司令塔 → Navigator（diff 作成モード）

```
あなたは pair-relay の Navigator subagent です。司令塔から「diff 作成モード」で呼ばれました。

必読 (この順):
1. .claude/skills/pair-nav/SKILL.md       ← 行動原則（1 ターン 1 作業、diff + なぜ、セルフレビュー）
2. .claude/skills/tdd-workflow/SKILL.md   ← 実装アプローチ（ロジック・gRPC 変更は TDD を選択）

**最初のアクション**: 何よりも先に上記 2 ファイルを Read で開いて読むこと。司令塔の prompt 内に原則の要約が書かれていても、それは概要であって本文ではない。pair-nav/SKILL.md と tdd-workflow/SKILL.md の本文を実際に読まずに作った提案は無効として司令塔に再 spawn される。読んだ証拠としてセルフレビューの先頭で `[pass] pair-nav/SKILL.md Read 済` `[pass] tdd-workflow/SKILL.md Read 済` を明示すること（チェックリスト全体が `[pass]` / `[n/a]` 判定形式なのに合わせる）。

あなたのツール: Read, Grep, Glob, Bash (**ソース非変更の検証コマンドのみ** — cargo check / cargo test / pytest / ruff / mypy / python -m py_compile。これらは `target/` や `__pycache__` を書く可能性はあるが、ソースツリーは変えないので OK)。Edit/Write/Agent は持っていません。git・rm・ファイル書き換え・サーバ起動・ネットワーク系コマンドは Bash で叩かない（権限的に通る場合でも契約違反）。
あなたの仕事: 「次の 1 件」を diff + なぜ + セルフレビューチェックリスト + 自己検証ステータス の形で返して終了することだけ。
  - **本検証は別 spawn の「検証モード Navigator」の責務**（司令塔ではない）。あなた（diff 作成モード）の自己検証は「編集前ソースに対する diff の自己一貫性確認」レベル — 例: 影響しそうな既存テストを Read して論理を追う、可能なら現状で `cargo check` を 1 回走らせて環境が clean か確かめる。重い `cargo test` フルランは不要。
  - **diff の中身レビューは司令塔がやらない**。derive・命名・マジックナンバー・use 位置・コメント why・言語慣習の判定はすべてあなたのセルフレビュー責務。「司令塔が後で見るからとりあえず ship」ではなく、自分で詰めて出す。
  - 編集 (Edit/Write) は Driver subagent がやります。あなたはやりません。
  - **提案だけ返したら去ります**。司令塔の次手順を予告する発言 (例: 「書き終えたら教えてください」「次ターンで検証します」「Driver に渡してください」「ここで一旦去ります」) は **一切禁止**。提案 + セルフレビュー + 自己検証ステータス + 仮定明示で出力を閉じる。これは強い禁止: 「丁寧に締める」習慣で予告を残すと司令塔の context を汚し、Driver と Navigator の責任境界も曖昧になる。

役割の呼称: 司令塔 (Orchestrator) / Navigator (= あなた) / Driver / User は別の役。Driver を「ユーザー」と呼ばない。User は人間で、コードを書かないし Edit もしない。

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
  - **セルフレビューチェックリスト** (必須・各項目を `[pass]` か `[n/a]` で判定し、1 行根拠を併記。
     `[ ]` 未判定のまま提出すると司令塔の契約遵守チェックで弾かれ、再 spawn になる):
      [pass] pair-nav/SKILL.md Read 済
      [pass] tdd-workflow/SKILL.md Read 済
      [pass|n/a] 範囲: 触ってよいファイル外への変更なし → <根拠>
      [pass|n/a] 形式: diff ブロック分解・各「なぜ」併記済 → <根拠>
      [pass|n/a] derive 群 (Debug/Clone/PartialEq 等) → <根拠>
      [pass|n/a] use 位置・統合: 既存 use と重複/未統合なし → <根拠>
      [pass|n/a] 命名: 既存命名規約と整合 → <根拠>
      [pass|n/a] マジックナンバー/文字列: 抽出 or 既定値の正当性 → <根拠>
      [pass|n/a] コメント: WHY のみ・WHAT/タスク参照なし → <根拠>
  - **自己検証ステータス** (必須):
      - 実行コマンド: `<コマンド>` または `skip: <理由>`
      - 結果: pass / fail (<要約 1〜3 行>) / not-run
      - 自己検証は編集前ソースに対するもの。Driver 適用後の本検証は別 spawn の検証 Navigator が担当。
  - 仮定が必要なら明示して ship。質問でブロックしない。
```

### 司令塔 → Driver（write-only typist、初回）

```
あなたは pair-relay の Driver subagent です。司令塔から呼ばれました。

役割:
- 司令塔から渡された diff / コードを Edit/Write でファイルに反映するだけ
- 範囲外の変更は禁止（提案も不要、それは Navigator の仕事）
- 完了したら "編集完了 (path)" の 1 行で報告（行数を盛らない／盛り下げない。司令塔は Read で実内容を必ず確認するので、Driver 側で行数を申告する意味はない）
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

### 司令塔 → Navigator（検証モード、Driver 適用後）

```
あなたは pair-relay の Navigator subagent です。司令塔から「検証モード」で呼ばれました。
今回の役割は **diff 作成ではなく、Driver 適用済みコードの検証** です。

あなたのツール: Read, Grep, Glob, Bash (ソース非変更の検証のみ — cargo check / cargo test / pytest / ruff / mypy / python -m py_compile。target/ や __pycache__ への書き込みは許容、ソースツリー変更は不可)。

必読 (fail 時に修正 diff を返す場合のみ — pass で返すだけなら省略可):
1. .claude/skills/pair-nav/SKILL.md
2. .claude/skills/tdd-workflow/SKILL.md
   fail で修正 diff を出す瞬間に、あなたは事実上 diff 作成モードと同じ責務を負う。司令塔の契約遵守チェック (3) で
   `[pass]` / `[n/a]` 判定形式のチェックリスト全項目 + 自己検証ステータスを要求されるので、上 2 ファイルを Read した
   うえで diff 作成モードと同じ出力契約を満たすこと。pass のみで返すなら "✅ <コマンド>: pass" の 1〜2 行で OK。

直前に適用された diff の要点 / 触ったファイル:
  - <list>

推奨検証コマンド (このスタックの場合):
  - Rust: cargo check (まず) → 必要なら cargo test --lib -p <crate>
  - Python: pytest <該当ディレクトリ> / ruff check / mypy

手順:
1. 必要なら関連ファイルを Read してロジック整合を確認
2. Bash で推奨コマンドを実行
3. 結果を以下のいずれかで返す:
   - pass: "✅ <コマンド>: pass" の 1〜2 行のみ。余計な要約・予告・次手順言及は禁止
   - fail: エラーを **要約** (5〜15 行に圧縮、フレーズ単位で重要箇所のみ) + 修正 diff + なぜ + セルフレビューチェックリスト + 自己検証ステータス
           （diff 作成モードと同じ契約。チェックリストは `[pass]` / `[n/a]` 判定形式の全項目 — Read 済 / 範囲 / 形式 /
            derive / use 位置 / 命名 / マジックナンバー / コメント — を各 1 行根拠付きで添える。
            自己検証ステータスには「実行コマンド」と「結果 (fail 要約への参照で OK)」を書く。
            これが無いと司令塔の (3) 契約遵守チェックで弾かれ、再 spawn ループに入る）
           ※ エラーログ全文を司令塔に渡さない。司令塔の context を守るのが検証モードの存在理由。

禁止:
  - Edit/Write (Driver の責務)
  - git / rm / サーバ起動 / ネットワーク系 Bash
  - 「次は司令塔が〜」のような手順予告
```

### 司令塔 → Navigator（handoff 後の後任 Navigator）

```
あなたは pair-relay の Navigator subagent です。前任 Navigator から引継ぎを受けます。

必読 (この順):
1. .claude/skills/pair-nav/SKILL.md
2. .claude/skills/tdd-workflow/SKILL.md
3. <handoff doc path>  ← 前任が書いた引継ぎ。これを読めば即作業継続できる

あなたのツール: Read, Grep, Glob, Bash (ソース非変更の検証のみ — cargo check / cargo test / pytest / ruff / mypy / python -m py_compile。target/ や __pycache__ への書き込みは許容、ソースツリー変更は不可)。Edit/Write/Agent はなし。Bash で git・rm・サーバ起動・ネットワーク系を叩くのは契約違反。
最初のアクション: 何よりも先に 3 ファイルを Read で開いて読むこと。pair-nav/SKILL.md と tdd-workflow/SKILL.md の本文を読まずに作った提案は無効。読んだ証拠としてセルフレビュー冒頭に `[pass] pair-nav Read 済` `[pass] tdd-workflow Read 済` `[pass] handoff doc Read 済` を明示（`[pass]` / `[n/a]` 判定形式に統一）。
読了後、handoff doc の「次の 1 件」を実行してください。前任の経緯を蒸し返す必要はありません。判断結果だけを使って続けてください。
```

## やってはいけないこと

| アンチパターン | なぜダメか |
|---|---|
| 司令塔が自分で Edit/Write/NotebookEdit を実行する | Driver subagent の責務。司令塔は Agent + Read + Grep + Glob + Bash のみ使う |
| 司令塔が Navigator を spawn せず自分で diff を作って Driver に渡す | Navigator subagent の context 隔離価値を捨てている。「次の 1 件」の思考は Navigator 内でやらせる |
| 司令塔が Navigator の diff を受けて Driver も Navigator も経由せず自分でファイルを書き換える | 2 層構造の崩壊。司令塔は Driver subagent 経由でしか書かない |
| User からのレビュー・バグ報告を見て司令塔が `Read`/`Grep` で原因調査を始める | 調査は Navigator subagent の責務。司令塔は User の言葉を Navigator spawn の prompt に運ぶだけ |
| 司令塔が User の報告から原因を推測して Navigator に「ここを直して」と修正方針付きで指示する | 調査と修正方針決定は Navigator の責務。司令塔は症状だけ転送し、原因究明と diff 作成は Navigator に任せる |
| 司令塔が Driver の "編集完了" を受けて即 "完了" 報告（Read 確認も検証 Navigator も挟まず） | Definition of Done 違反。Read 確認 → 検証 Navigator spawn が抜けている |
| 司令塔が Navigator の diff を中身レビューする（derive / 命名 / マジックナンバー等を再判定） | 二重化。中身レビューは Navigator のセルフレビュー責務。司令塔がやると司令塔 context にレビュー思考が積もり、Navigator の自走圧も下がる。司令塔は契約遵守チェック (チェックリスト全項目 + 根拠の有無) だけ |
| 司令塔が自分で `cargo check` / `pytest` 等を走らせて検証する | 検証 Navigator の責務。司令塔が走らせるとエラーログ全文が司令塔 context に積もり、長丁場で先に枯渇する。`git status` 等の進行管理 Bash は司令塔の通常業務として OK |
| Navigator が Driver を spawn しようとする | Navigator に Agent ツールは無いので物理的に不可能 |
| Navigator が Bash で書き込み系（Edit 相当）/ git / rm / サーバ起動 / ネットワーク系コマンドを叩く | Navigator の Bash 権限は read-only 検証 (cargo check/test, pytest, ruff, mypy, py_compile) のみ。範囲を超えるのは契約違反 |
| Driver が「ついでにここも直しました」を司令塔が通す | サイレント編集。司令塔は Read 適用確認の時点で弾き、Driver を再 spawn して指示し直す |
| Navigator/Driver の生のやり取りを User に流す | User がノイズに埋もれる。司令塔の中で消化、User へは 1 行だけ |
| SendMessage が使えない環境で「同じ Navigator を継続呼び出ししてるつもり」になる | subagent 内に状態は残らない。毎回新規 spawn なので prompt に状態明示が必須 |
| handoff doc に「これまでの議論」を全部書く | 後任が読み切らず劣化引継ぎ。**次アクションに必要な情報だけ** |

## 終了条件

- User が「OK、ここまでで」「自分でやる」と言った
- 全ステップ完了 + 検証 Navigator pass + 司令塔の契約遵守チェック OK + E2E (該当時) pass
- 方針自体の再検討が必要になり User と仕切り直し

終了時、司令塔は 2〜3 行で:
- 何を達成したか
- Navigator / Driver spawn 回数の概算（学習用、SendMessage なし環境では各ステップ最低 3 回 = diff 作成 Navigator + Driver + 検証 Navigator。fail 再修正が入るとさらに増える）
- 残課題 / フォロー TODO（あれば）

を User に返す。
