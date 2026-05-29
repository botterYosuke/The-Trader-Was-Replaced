---
name: pair-relay
description: 司令塔Agentが Navigator と Driver を分けて長い実装を進めるための運用スキル。司令塔は実装・レビュー・検証を自分で抱えず、Driver の完了報告やバグ報告を Navigator に渡し、Navigator の「次の 1 手」を Driver に渡す。Driver は .claude/agents/pair-relay-driver.md、Navigator は .claude/agents/pair-relay-navigator.md に従う。トリガー: 「/pair-relay」「計画書を実装してください」「プランを実装」「段階的に実装」「複数ファイルにまたがる実装」「長い実装」「フェーズを実装」「TDD で実装」「codex でレビューして修正」「レビューして Medium 以上が無くなるまで修正」「コードレビューしてバグを直して」と言われたとき。複数ファイル・複数レイヤー（Rust + Python）にまたがる実装や、プランファイルを渡されて実装を指示されたときに優先的に発動すること。⚠️ **コード実装だけでなく「実装前のプラン／issue レビュー＆修正ループ」も本スキルの対象**: 「実装前にこの計画をレビューして修正」「issue/プランを Navigator と codex にレビューさせて Medium 以上が無くなるまで直す」と言われたら、Navigator がコードと突き合わせてプラン改訂を起草 → codex を独立セカンドオピニオンとして並走 → 司令塔が Medium 以上を集約 → Navigator が改訂版プランを再起草 → Medium 以上ゼロまでラウンドを回す。この場合 Driver の「適用」対象はコードではなく **issue 本文／プランファイルの編集**（`gh issue edit --body-file` 等）になる。プランがコード（footer.rs / main.rs / proto / tests）と食い違う指摘は実装着手前に潰せるので、コストの高い手戻りを防げる。⚠️ src/ui/** を触る作業では Navigator spawn 前に必ず bevy-engine スキルを発動して内容を把握すること（読まずに進めると Bevy 0.15 固有の罠でハマる）。⚠️ Rust テスト（`#[cfg(test)] mod tests` 追加や `cargo test --lib` 主体の TDD ループ）を伴う作業では Navigator spawn 前に rust-testing スキルも発動すること（subagent は親が発動したスキルしか引き継がない）。⚠️ Python テスト（pytest、特に `pytest-httpx` / `pytest-asyncio` / `freezegun` を使う RED→GREEN ループ）を伴う作業では Navigator spawn 前に tdd スキルも発動すること。**「バグ修正」「issue を実装」「不具合修正」名目でも、CLAUDE.md が「先に RED テストを書いてから修正」を要求している場合は必ず tdd を発動する**（実例: #39 issue 実装で grpc pytest RED→GREEN が発生したが tdd を発動せず—ラベルが『修正』でも RED→GREEN が本質なので発動すべきだった）。⚠️ 立花証券・kabuステーション venue 関連の Python 実装では tachibana / kabusapi スキルも事前発動すること（API 規約 R1-R10 を Navigator が踏まないため）。⚠️ Nautilus の exec client / order イベント（`_modify_order`・`generate_order_canceled/expired/filled/updated/modify_rejected`・OrderStatus FSM・PENDING_UPDATE・OrderModifyRejected）を触る Python 実装では Navigator spawn 前に nautilus-trader スキルも発動すること（Navigator が毎回 `.venv` の Nautilus ソースを読み直さず、シグネチャ/状態遷移を skill mirror で裏取りできる）。⚠️ これらの条件付きスキル発動（bevy-engine / rust-testing / tdd / tachibana / kabusapi / nautilus-trader）は **session 冒頭だけでなく slice ごとに再判定**すること。複数 slice を跨ぐ長い実装では、後続 slice で初めて Rust テスト・src/ui・Python テスト・venue 領域に入ることがある。その slice の Navigator spawn 直前に、該当スキルを未発動なら司令塔の責任で発動する（例: Python slice を数本こなした後 Rust seam テスト slice に入るなら、その Navigator spawn 前に rust-testing を発動）。冒頭で /pair-relay /tdd だけ指定されていても、Rust テスト slice に到達したら rust-testing を足すのは司令塔の責任。⚠️ これは新規『実装』だけでなく **レビュー後修正（review-fix / 「Medium が無くなるまで修正」）ループでも同じ**: 修正が pytest の RED→GREEN や `#[cfg(test)]`/seam assert の追加を伴えば、タスクが『レビュー』『修正』名目でも rust-testing / tdd を発動する。トリガーは Rust/Python テスト領域・src/ui・venue に入ること自体であり、『実装 vs 修正』のラベルではない（#29 Slice1 のレビュー修正で pytest 回帰 + Rust seam assert を足したのに rust-testing/tdd を発動し忘れた実例あり）。⚠️ context 消費に注意: Driver/Navigator 1 往復で 25k+ tokens 消費するため、1 session で消化できる subtask は 2-3 個が現実的。プランファイル全消化を 1 session で狙わない。完了後（コード変更がある場合）は code-review(simplify) スキルで変更コードをレビューすること（standalone な `simplify` スキルは存在しない）。⚠️ **機能追加・挙動変更を含む実装が完了したら、CLAUDE.md の規約に従い behavior-to-e2e を必ず併発すること**（FLOWS.md への flow note 追加 + wiki `[FlowID]` 引用）。「リファクタのみ」でも新しい不変条件（drain 順序保証など）が生まれた場合は発動する。実例（#46 Slice E）: drain_keyboard 新機能追加後に behavior-to-e2e を発動せず FLOWS.md/wiki 更新が漏れた。⚠️ **Rust TDD slice（`#[cfg(test)] mod tests` を新規追加し `cargo test --lib` で RED→GREEN ループを回す）では必ず tdd スキルを invoke すること**（pair-relay 内で TDD を実施しても、tdd スキルを invoke しないと `#[serial]` / `bevy::log::warn!` vs `tracing::warn!` の caveat チェックが抜ける）。実例（#46 Slice E）: ユーザーが /tdd を明示したにもかかわらず invoke せず。
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

## 「Medium 以上なし」を確定する前に必ずテストを回す

レビュー＆修正ループ（「Medium が無くなるまで」）で Navigator が **コードレビューだけで「Medium 以上なし」と結論しても、司令塔はそれを最終確定にしない**。コードレビューは静的観察であり、実際に落ちるテストを見落とすことがある。

ルール:

- Navigator が「Medium 以上なし」と返したら、司令塔は **完了宣言の前に必ず該当範囲の `cargo test` / `pytest` を回す**（Navigator/Driver に Bash が無い harness では司令塔が代行する原則どおり）。回す対象は Navigator が起草した検証コマンド、無ければ変更ファイルに対応するユニット/統合テスト。
- テストが赤なら、その raw 出力を **判定せずに Navigator へ verbatim で渡す**。Navigator が「実装バグ（Medium 以上）か / stale テスト（実装は正・テストが誤り）か」を切り分け、次の 1 手を起草する。
- 「コミットメッセージに GREEN と書いてある」「前任 Navigator が健全と言った」だけで緑とみなさない。実際に回して確かめる。

実例（issue #64 レビュー）: Navigator はコードレビューのみで「Medium 以上なし」と結論したが、司令塔が裏取りで `pytest` を回したところ 1 件が赤（`assert success is False` だが実装は `True` を返す）。Navigator 再判定で「register は mode gate なし＝実装が正、テストの期待が stale」と切り分けられ、テスト側を直して解決した。**司令塔がテストを回さなければこの赤は完了報告をすり抜けていた。**

⚠️ **独立セカンドオピニオン（codex 代替の Claude レビュー Agent）は必ず read-only agent type（`Explore`）で spawn する**。`general-purpose` は Bash/Edit/Write を持つため、レビュー中に作業ツリーを触りうる。実例（#46 Slice B2 Step7）: `general-purpose` のレビュー Agent が「baseline 比較」のため `git stash pop` を実行 → conflict → `git reset --hard` で復旧、という**破壊的 git をレビュー名目で実行**した（幸い tree/stash は無傷で復旧）。レビューは静的観察に徹するべきで、`Explore`（read-only）なら構造的に不可能になる。cargo/git の裏取りは従来どおり司令塔が代行し raw 出力を渡す。これは「Navigator/Driver に破壊的 git をやらせない」原則の **レビュー Agent への拡張**。

## 完了報告（司令塔 → Human）

完了時、司令塔Agentは事実を短く並べます。

```text
完了です。

- cargo check: OK
- cargo test: OK
- E2E Step 1-8: PASS
- 旧保存経路 grep: 残骸なし
- 本実装差分: Cargo.toml / Cargo.lock / src/ui/...
- 検証副作用: examples/test_strategy_daily.* は戻す候補
```

## 司令塔Agentの禁止事項（一覧）

司令塔は次のいずれも **やらない**。やりたくなる場面ほど踏みとどまる。

- 自分で実装する（Edit / Write）
- 自分で設計判断する（「たぶん」「想定通り」「進めて OK」）
- 自分でコードレビューする
- 自分で `cargo check` / `cargo test` / `rg` / `Read` を走らせる（**例外**: Navigator/Driver に Bash が付与されない harness では検証コマンドは司令塔が走らせ、**raw 出力をそのまま Navigator に渡して解釈させる**。下記「Navigator/Driver に Bash が無い harness の運用」参照）
- Navigator の指示を要約・改変・順番入れ替え
- Driver の警告・確認質問・review-block を省略・丸めて Navigator に渡す
- respawn 時に引き継ぎを書き換える・追加する・要約する
- Driver の確認質問に GO/NO-GO を直接返す
- バグ仮説を採用して Driver に直接修正させる
- 失敗ログを丸ごと Human に流す
- E2E 結果を曖昧にする
- 無関係差分をまとめて戻す
- `git reset --hard` を提案する
- **Navigator/Driver に破壊的 git 操作を許す**（下記「未コミット作業の保護」）

例外として許されるのは、Human への最終報告での **重複ログ圧縮** のみ。作業判断に関わる情報は削らない。

## 未コミット作業の保護（破壊的 git は subagent に絶対やらせない）

実例: ある Navigator が一時 probe テストを「`git checkout` で除去」した際、**同ファイルの未コミット追加（289 行のテスト）まで巻き戻して消失**させた。実装は別ファイルで無傷だったが、消えたテストは未コミット＝git に記録が無く、復旧元は会話履歴の verbatim だけだった。

ルール:

- **Navigator/Driver を spawn する prompt に必ず**「`git checkout` / `git reset` / `git restore` / `git stash` 等、作業ツリーを巻き戻すコマンドは実行禁止」と明記する。検証は pytest / cargo / Read / Grep / `git status` まで。
- 一時 probe コードが要るなら、別 throwaway ファイルに書くか Read だけで観測し、消すときも対象を名指しの Edit で消す（git で戻さない）。
- subagent はワークツリー全体の文脈を持たない。`git checkout <file>` でも、その subagent が作っていない未コミット変更まで道連れにする。`git reset --hard` だけの問題ではない。
- 長い実装で未コミット差分が積み上がったら、節目で WIP コミットして保護を検討する（git は Bash 経由）。

## SendMessage が unavailable な harness の運用

理想は Navigator を 1 個体に保ち、SendMessage で複数往復を回すこと（context が積み上がる）。ただし harness によっては `SendMessage` が deferred tool 一覧に出ない (= 利用不可) ことがある。その場合の代替手順:

1. Navigator は毎ターン **新個体を spawn** する（前 Navigator の agentId は使えないし諦める）
2. 司令塔は新 Navigator の prompt に以下を必ず含める:
   - 「前任が起草した RED/差分は Driver により観測済み」の 1 行
   - Driver の完了報告を **verbatim**（要約・改変なし、code block で括る）
   - 直前の commit sha と現在の HEAD
   - regression baseline 数字（passed/failed/errors）
   - これまでに固まったユーザー確定仕様（B4 全体仕様など、複数 subtask に跨る決定事項）
3. Navigator の最初の仕事に「Read で SUT 直前状態を確認してから次手を起草」を含める（記憶がないため）

この運用で 4-5 ターン回しても context 整合は保てる。同個体継続より 1 ターンあたり 5-10k token 増えるので、1 session あたりの subtask 上限は 2-3 のままで変わらない。

**ただし「1 つの大タスクを小ステップに割って 15-20+ ターン回す」のは別物で、可能**（実績: Phase 7.3 A0 の per-instrument data refactor を proto→python→rust の十数ステップで完走）。鍵は **司令塔が cross-turn state を保持すること**: 毎回の新 Navigator prompt に「**安定した step シーケンス全体 + ユーザ確定の binding decisions + 直近の baseline 数字**」のコンパクトな block を貼り続ける（Navigator 個体は記憶を持たないが、この block が "外部記憶" になる）。各ステップを「1 ファイル / 1 関数 / 1 論理修正」まで小さく保てば、Navigator 1 個体の context は毎回ほぼ空から始まり破綻しない。subtask 上限 2-3 は「Navigator が跨いで判断を積む必要がある独立タスク」の話で、「司令塔が直列に運ぶ小ステップ」には適用されない。

## Navigator/Driver に Bash が無い harness の運用

harness によっては subagent (Navigator / Driver) に **Bash が付与されない**ことがある（Navigator は Read/Grep/Glob のみ、Driver は Read/Edit/Write のみ。agent 定義に `Bash` が書いてあっても実行時に剥がされる）。この場合 `pytest` / `cargo` / `rg` などの**検証を誰も走らせられない**ので、**司令塔が代行する**（上記禁止事項の例外）。要点:

1. **Navigator は検証コマンドを「起草」する**（実行はしない）。「司令塔が cwd=X で `<cmd>` を回してください、期待は Y」の形で次手に添える。
2. **Driver は Edit/Write のみ**。Navigator が依頼した Bash も実行できないので、編集だけさせる（Bash 込みの 1 手を渡すと Driver が `[review-block]` で返す → Bash 部分を司令塔に巻き取って Edit のみ再依頼）。
3. **司令塔が検証コマンドを実行し、raw 出力（要点）を次の Navigator に verbatim で渡す**。司令塔は合否を**自分で判定しない**（「3 passed」「`WinError 10038` で 4 failed」等の事実だけ運ぶ。緑/赤の意味づけ・次手は Navigator）。コマンド実行は機械作業、解釈は Navigator —「司令塔は判断しない」原則と矛盾しない。
4. RED→GREEN は「司令塔が RED 確認 → Driver が実装 Write → 司令塔が GREEN 確認 → 次 Navigator が解釈」で回す。
5. proto/codegen の生成物 fixup（例: `engine_pb2_grpc.py` の import 再パッチ）も Edit なので Driver に渡す（司令塔が直接 Edit しない）。codegen コマンド自体は司令塔が走らせる。
6. この harness では Navigator/Driver が**最初の 1-2 ターンで「Bash を持っていない」と返して判明する**。判明したら即この運用に切り替える。

## 合言葉

運ぶ。混ぜない。削らない。急がせない。  
Navigator には判断を、Driver には 1 手を、Human には事実を渡す。
