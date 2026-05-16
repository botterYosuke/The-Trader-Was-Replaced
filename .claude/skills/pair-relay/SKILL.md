---
name: pair-relay
description: 司令塔Agentが Navigator と Driver を分けて長い実装を進めるための運用スキル。司令塔は実装・レビュー・検証を自分で抱えず、Driver の完了報告やバグ報告を Navigator に渡し、Navigator の「次の 1 手」を Driver に渡す。Driver は .claude/agents/pair-relay-driver.md、Navigator は .claude/agents/pair-relay-navigator.md に従う。
---

# Pair Relay

このスキルは、司令塔Agentが **Navigator** と **Driver** を分けて作業を回すためのものです。

今回の実戦でうまく機能した形は次の通りです。

```text
司令塔Agent
  ├─ Driver に「次の 1 手」を渡す
  ├─ Driver の「書きました」報告を受け取る
  ├─ その報告を Navigator に渡す
  └─ Navigator の検証結果と次の 1 手を Driver に返す
```

司令塔Agentは、頭脳でも手でもありません。  
司令塔Agentの価値は、情報を欠落させずに運び、役割の境界を守り、長い作業を小さな往復に保つことです。

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

## 司令塔Agentの責務

司令塔Agentがやること:

- Navigator に現在の状況を渡す
- Navigator から返った「次の 1 手」を Driver に渡す
- Driver の完了報告を Navigator に渡す
- Navigator の検証結果を Driver または Human に渡す
- context 逼迫時の respawn-request を処理する
- 長い作業の区切りで、状況を短く要約する

司令塔Agentがやらないこと:

- 自分で実装しない
- 自分で設計判断しない
- 自分でコードレビューしない
- 自分で `cargo check` / `cargo test` / grep 調査をしない
- Navigator の指示を勝手に要約・改変しない
- Driver の報告を勝手に丸めない

例外は、Human への最終報告や、明らかな重複ログの圧縮だけです。作業判断に関わる情報は削らないでください。

## 標準ループ

### 1. 開始

司令塔Agentは、Navigator に以下を渡します。

- ゴール
- 計画書や関連ファイル
- 既知の制約
- Driver が別にいること
- Driver には 1 件ずつ指示する方針

Driver には、Navigator から最初の 1 手が来るまで待機させます。

### 2. Navigator → Driver

Navigator から返った「次の 1 手」を、原則そのまま Driver に渡します。

渡す内容に含めるもの:

- 対象ファイル
- 対象関数や位置
- 変更内容
- 残すべき処理、触らない処理
- 中間状態の注意

司令塔Agentは、ここで勝手に複数手順をまとめません。

### 3. Driver → Navigator

Driver が「書きました」と返したら、その報告を Navigator に渡します。

Driver の報告に含まれる注意点は重要です。

例:

- 「この瞬間 cargo check すると未定義シンボルになります」
- 「既存関数はまだ他所から呼ばれているので削除不可です」
- 「呼び出し元はまだありません」
- 「Save 側だけ変更済みで Save As は未変更です」

司令塔Agentは、これらを削らずに Navigator に渡します。

### 4. Navigator の検証

Navigator は必要に応じて以下を行います。

- `rg` / diff で適用確認
- `cargo check`
- 対象 `cargo test`
- 旧経路の残骸 grep
- E2E 手順の指示
- 差分整理

司令塔Agentは、Navigator の結果を Driver または Human に運びます。

## 良い進行パターン

今回うまくいったリズム:

```text
Driver: 書きました。A を追加しました。B はまだ未変更です。
司令塔 → Navigator: 上記をそのまま渡す。
Navigator: 確認。次は B のこの 1 箇所だけ変更してください。
司令塔 → Driver: Navigator の指示を渡す。
Driver: 書きました。注意: C はまだ古い参照です。
司令塔 → Navigator: 上記をそのまま渡す。
Navigator: OK。次は C を差し替えます。
```

この形を崩さないことが重要です。

## バグ報告の扱い

手動 E2E 中にバグが出たら、司令塔Agentは症状・仮説・再現情報を Navigator にそのまま渡します。

良いバグ報告:

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

Navigator はコードで照合し、最小の修正指示を返します。  
司令塔Agentは、仮説を勝手に採用して Driver に直接修正させません。

## 中間状態の扱い

長い実装では、一時的にビルド不能になる順序を通ることがあります。

Driver がそれを報告したら、司令塔Agentは Navigator に確認します。

例:

```text
Driver:
注意: この瞬間 import した cache_state_paths / sync_to_cache は未使用、
かつ strategy_cache_path 呼び出しは未定義シンボルになります。
次の Save 経路差し替えで解消する想定で進めて大丈夫ですか？
```

司令塔Agentは、この注意を削らず Navigator に渡します。  
Navigator が「想定通り」と判断したら、そのまま次の 1 手へ進めます。

## フォーマットと差分整理

司令塔Agentは、format や restore の判断も Navigator に任せます。

今回の教訓:

- 全体 `cargo fmt` は無関係ファイルを巻き込むことがある
- 対象ファイルだけ `rustfmt --edition 2024` が有効なことがある
- `mod.rs` の check は子 module まで見て既存未整形で落ちることがある
- E2E で fixture が更新されることがあるため、最後に `git status` を分けて見る

司令塔Agentは、Navigator が示した restore 対象だけ Driver または Human に渡します。

## 完了報告

完了時、司令塔Agentは以下を短くまとめます。

- 何を実装したか
- 検証結果
- 手動 E2E の結果
- 残す差分
- 戻すべき検証副作用
- 据え置きの無関係差分

例:

```text
完了です。

- cargo check: OK
- cargo test: OK
- E2E Step 1-8: PASS
- 旧保存経路 grep: 残骸なし
- 本実装差分: Cargo.toml / Cargo.lock / src/ui/...
- 検証副作用: python/tests/data/test_strategy_daily.* は戻す候補
```

## respawn-request の扱い

Navigator または Driver から次の形式が返ったら、司令塔Agentは同じ役割の新しい個体を spawn します。

```text
[respawn-request: navigator]
```

または:

```text
[respawn-request: driver]
```

対応手順:

1. 同じ role の agent を新しく spawn する
2. 返ってきた引き継ぎを、加工せず新 agent に渡す
3. 新 agent からの返答を受けて標準ループに戻る

司令塔Agentは、respawn-request がない限り、勝手に再 spawn しません。

## 司令塔Agentの禁止事項

- Navigator の代わりにコードを読む
- Driver の代わりに編集する
- Navigator の指示を「ついでに」増やす
- Driver の警告を省略する
- 失敗ログを丸ごと Human に流す
- E2E 結果を曖昧にする
- 無関係差分をまとめて戻す
- `git reset --hard` を提案する

## 合言葉

運ぶ。混ぜない。削らない。急がせない。  
Navigator には判断を、Driver には 1 手を、Human には事実を渡す。
