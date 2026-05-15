# Handoff Document Template

Navigator subagent / Driver subagent / 司令塔 のいずれの引継ぎにも使う共用テンプレ。退役する側、または司令塔が長丁場で context を整理するときに書く。後任が **これだけ読めば即座に続けられる** こと。冗長な経緯禁止。**判断結果と次アクションだけ** 残す。

なお pair-relay は 2 層構造（司令塔が Navigator と Driver を順次 spawn）であり、subagent 側に状態は残らない（SendMessage 利用可否で挙動分岐）。SendMessage が使えない環境ではこの handoff doc が「subagent 間の状態引き継ぎ」の唯一の手段になる。

```markdown
# Handoff: <Orchestrator|Navigator|Driver> @ <YYYY-MM-DD HH:MM>

## 1. ゴール (全体)
<1〜2 行。User のゴールをそのまま、または要約>

## 2. 完了済みステップ
- [x] Step 1: <短く>
- [x] Step 2: <短く>
- [ ] Step 3: 進行中 ← いま <作業位置>
- [ ] Step 4: 未着手
- [ ] Step 5: 未着手

## 3. 次の 1 件 (後任が真っ先にやる作業)
<司令塔自身の引継ぎ → 次に Navigator subagent に何を聞くか / Driver subagent に何を渡すか の 2 系統を明示>
<Navigator 引継ぎ → 次に出すべき diff (+ なぜ) の方向性、または「司令塔の検証結果待ち」>
<Driver 引継ぎ → 次に適用する diff (司令塔がまだ渡していなければ「司令塔の指示待ち」)>

## 4. 触ったファイルと現状
| path | 状態 | 直近検証 |
|---|---|---|
| src/foo.rs | dirty (Step 2 で追加) | cargo check pass @ <時刻> |
| src/bar.rs | clean | - |

## 4b. Subagent 再開方針 (司令塔 handoff 時のみ)
- Navigator: <SendMessage で継続再利用する / 毎回新規 spawn する>
- Driver: <SendMessage で継続再利用する / 毎回新規 spawn する（既定）>

## 5. 既知の落とし穴 / User からの制約
- <制約 1>
- <落とし穴 1>

## 6. やらないこと（明示的に範囲外）
- <範囲外 1>
- <範囲外 2>

## 7. この場面で特に効く pair-nav 原則
- <例: 既存ファイル修正中なので diff ブロック + 各「なぜ」厳守>
- <例: Bevy 0.14 を仮定して書いている。version 質問でブロックしない>
```

## 書くときの注意

- **経緯を書かない**: 「最初は X だったが Y に変えた」のような議事録は不要。決定事項だけ。
- **行数の目安**: 全体で 40〜80 行。それ以上は読まれない。
- **「次の 1 件」が一番大事**: ここが曖昧だと後任が迷う。具体コードや具体ファイル名を入れる。
- **検証状態の鮮度**: 「直近検証」は時刻付き。古ければ後任が自分でもう 1 回走らせる判断ができる。
