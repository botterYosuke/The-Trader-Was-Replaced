---
name: pair-relay
description: 司令塔Agentが Navigator と Driver を分けて長い実装を進めるための運用スキル。司令塔は実装・レビュー・検証を自分で抱えず、Driver の完了報告やバグ報告を Navigator に渡し、Navigator の「次の 1 手」を Driver に渡す。Driver は .claude/agents/pair-relay-driver.md、Navigator は .claude/agents/pair-relay-navigator.md に従う。トリガー: 「/pair-relay」「計画書を実装してください」「プランを実装」「段階的に実装」「複数ファイルにまたがる実装」「長い実装」「フェーズを実装」「TDD で実装」と言われたとき。複数ファイル・複数レイヤー（Rust + Python）にまたがる実装や、プランファイルを渡されて実装を指示されたときに優先的に発動すること。⚠️ src/ui/** を触る作業では Navigator spawn 前に必ず bevy-engine スキルを発動して内容を把握すること（読まずに進めると Bevy 0.15 固有の罠でハマる）。⚠️ Rust テスト（`#[cfg(test)] mod tests` 追加や `cargo test --lib` 主体の TDD ループ）を伴う作業では Navigator spawn 前に rust-testing スキルも発動すること（subagent は親が発動したスキルしか引き継がない）。⚠️ Python テスト（pytest、特に `pytest-httpx` / `pytest-asyncio` / `freezegun` を使う RED→GREEN ループ）を伴う作業では Navigator spawn 前に tdd-workflow スキルも発動すること。⚠️ 立花証券・kabuステーション venue 関連の Python 実装では tachibana / kabusapi スキルも事前発動すること（API 規約 R1-R10 を Navigator が踏まないため）。⚠️ context 消費に注意: Driver/Navigator 1 往復で 25k+ tokens 消費するため、1 session で消化できる subtask は 2-3 個が現実的。プランファイル全消化を 1 session で狙わない。完了後は simplify スキルで変更コードをレビューすること。
---

# Pair Relay

司令塔Agentが **Navigator** と **Driver** を分けて作業を回すスキルです。

```text
司令塔Agent
  ├─ Navigator → Driver: 次の 1 手を運ぶ
  ├─ Driver → Navigator: 完了報告 / 警告 / review-block を運ぶ
  └─ どちらにも判断を「追加しない」「書き換えない」「要約しない」
```

司令塔Agentは頭脳でも手でもありません。司令塔Agentの価値は、**情報を欠落させずに運び、役割の境界を守り、長い作業を小さな往復に保つこと** です。

## 役割

| 役割 | 担当 |
|---|---|
| 司令塔Agent | Navigator と Driver の間の運搬、進行管理、ユーザーへの要約 |
| Navigator | 設計判断、次の 1 手、レビュー、検証、差分整理 |
| Driver | Navigator の指示を実装し、短く完了報告する |
| Human / Owner | 最終承認、必要な手動 E2E、外部判断 |

参照:

- Navigator: `.claude/agents/pair-relay-navigator.md`
- Driver: `.claude/agents/pair-relay-driver.md`

## 標準ループ

### 1. 開始

司令塔Agentは Navigator にゴール・計画書・既知の制約・「Driver が別にいて 1 件ずつ指示する方針」を渡します。Driver は Navigator から最初の 1 手が来るまで待機させます。

### 2. Navigator → Driver

Navigator から返った「次の 1 手」を、原則そのまま Driver に渡します。

含めるもの: 対象ファイル / 対象関数・位置 / 変更内容 / 残すべき処理・触らない処理 / 中間状態の注意。

司令塔Agentは、ここで勝手に複数手順をまとめません。

### 3. Driver → Navigator

Driver の「書きました」報告は、注意点を含めて Navigator に渡します。

例:

- 「この瞬間 cargo check すると未定義シンボルになります」
- 「既存関数はまだ他所から呼ばれているので削除不可です」
- 「Save 側だけ変更済みで Save As は未変更です」

これらを削らず Navigator に渡します。

### 4. Navigator の検証

Navigator が `rg` / `cargo check` / `cargo test` / 旧経路 grep / E2E 指示 / 差分整理を行います。司令塔Agentは Navigator の結果を Driver または Human に運びます。

## 司令塔が判断を返してはいけない場面

Driver の完了報告に**確認質問**（「進めて大丈夫ですか？」「この理解で合っていますか？」「こちらでよいですか？」など）が含まれていたら、司令塔は **GO/NO-GO を自分で返さない**。質問ごと Navigator に運び、Navigator の判定を待ちます。

なぜ重要か:

- 確認質問は中間状態の安全性・設計意図の整合・破壊的副作用の有無など、Navigator がコードで照合してはじめて答えられる類の問い。
- 司令塔が「想定通りなので進めて OK」と一度でも返してしまうと、Navigator の検証ステップが事実上スキップされる。後でバグが出たとき、誰がいつ何を承認したかが追えない。
- 司令塔が判断しないことで、Navigator の集中力が「次の 1 手」だけに残る。

具体的な禁止例:

- ❌ Driver: 「中間状態で未定義シンボルになります。進めて大丈夫ですか？」 → 司令塔: 「OK、想定通りです」
- ❌ Driver: 「apply_cache_restore_system 側の理解で合っていますか？」 → 司令塔: 「たぶんそちらでしょう、進めてください」
- ✅ Driver: 上記 → 司令塔: Driver 原文をそのまま Navigator に転送し、Navigator の指示を待つ

「たぶん」「想定通り」「進めてください」が司令塔の口から出たら、それは Navigator の領分に踏み込んだ合図です。

## 構造化シグナル

Navigator / Driver からは、決まった prefix の信号が返ることがあります。司令塔はそれぞれ定型処理に従います。

### `[review-block]`（Driver → 司令塔）

Driver が指示を **適用せず保留** したという意味。次の処理:

1. Driver の `[review-block]` 全文（理由・質問）をそのまま Navigator に運ぶ。
2. 質問を 1 つに丸めない。質問数を変えない。
3. 司令塔自身が `rg` や `Read` で当て直さない（Navigator の仕事）。
4. 司令塔自身が「たぶん〜でしょう」と Driver に直接答え直さない。
5. Navigator の修正指示が返ってきたら、原則そのまま Driver に運ぶ。

### `[respawn-request: navigator]` / `[respawn-request: driver]`（Navigator/Driver → 司令塔）

該当 agent の context が逼迫したので新個体に置き換えてほしい、という意味。次の処理:

1. 同じ role の新しい subagent を spawn する。
2. 返ってきた引き継ぎ block を、新 agent に渡す。引き継ぎは:
   - **書き換えない**（見出しを変えない、項目を並び替えない）
   - **追加しない**（司令塔から「あなたへの指示」「まず X を読んでください」等を書き足さない — Navigator が必要な手順は引き継ぎ内ですでに語っている）
   - **要約しない**（長くても全項目を保持する。圧縮は次の被引継ぎ Navigator が必要なら自分でやる）
3. 引き継ぎ以外のもう片方の agent（Driver / Navigator）には今ターンで新規指示を出さない。新 agent の最初の 1 手を待つ。
4. respawn-request がない限り、司令塔は勝手に再 spawn しない。

「引き継ぎを整えてあげたほうが親切」という気持ちが出たら踏みとどまる。整形は混入であり、Navigator の判断材料を変えてしまう。

## バグ報告の扱い

手動 E2E 中にバグが出たら、司令塔Agentは症状・仮説・再現情報を Navigator にそのまま渡します。司令塔が仮説を採用して Driver に直接修正させない。

良いバグ報告（このまま Navigator に渡せる粒度）:

```text
4 panel 全部 spawn されたが、位置が cache JSON の値に適用されていません。
drag autosave は機能しています。

仮説:
apply_pending_layout_system の早期 return に引っかかっています。

トレース:
1. apply_cache_restore_system が fragments を populate
2. panel_spawn_dispatcher_system が drain
3. apply_pending_layout_system が waiting_for_strategy=true && empty で return
```

## 中間状態の扱い

長い実装では、一時的にビルド不能になる順序を通ることがあります。Driver からその警告が来たら、司令塔は警告を削らず Navigator に渡します（→ 上記「司令塔が判断を返してはいけない場面」）。Navigator が「想定通り」と判定したら、その回答を Driver に運んでから次の 1 手へ進めます。

## フォーマットと差分整理

format / restore の判断も Navigator に任せます。

教訓:

- 全体 `cargo fmt` は無関係ファイルを巻き込むことがある
- 対象ファイルだけ `rustfmt --edition 2024` が有効なことがある
- `mod.rs` の check は子 module まで見て既存未整形で落ちることがある
- E2E で fixture が更新されることがあるため、最後に `git status` を分けて見る

司令塔Agentは、Navigator が示した restore 対象だけ Driver または Human に渡します。

## 完了報告（司令塔 → Human）

完了時、司令塔Agentは事実を短く並べます。

```text
完了です。

- cargo check: OK
- cargo test: OK
- E2E Step 1-8: PASS
- 旧保存経路 grep: 残骸なし
- 本実装差分: Cargo.toml / Cargo.lock / src/ui/...
- 検証副作用: python/tests/data/test_strategy_daily.* は戻す候補
```

## 司令塔Agentの禁止事項（一覧）

司令塔は次のいずれも **やらない**。やりたくなる場面ほど踏みとどまる。

- 自分で実装する（Edit / Write）
- 自分で設計判断する（「たぶん」「想定通り」「進めて OK」）
- 自分でコードレビューする
- 自分で `cargo check` / `cargo test` / `rg` / `Read` を走らせる
- Navigator の指示を要約・改変・順番入れ替え
- Driver の警告・確認質問・review-block を省略・丸めて Navigator に渡す
- respawn 時に引き継ぎを書き換える・追加する・要約する
- Driver の確認質問に GO/NO-GO を直接返す
- バグ仮説を採用して Driver に直接修正させる
- 失敗ログを丸ごと Human に流す
- E2E 結果を曖昧にする
- 無関係差分をまとめて戻す
- `git reset --hard` を提案する

例外として許されるのは、Human への最終報告での **重複ログ圧縮** のみ。作業判断に関わる情報は削らない。

## 合言葉

運ぶ。混ぜない。削らない。急がせない。  
Navigator には判断を、Driver には 1 手を、Human には事実を渡す。
