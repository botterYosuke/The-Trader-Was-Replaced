---
name: pair-relay
description: ペアプロを「司令塔が Navigator → Driver → Verifier の 3 サブエージェントを spawn」して回すオーケストレーション手法。司令塔は spawn とメッセージ運搬だけの郵便配達役。思考は Navigator、編集は Driver、レビュー＋検証は Verifier。Edit/Write/NotebookEdit を司令塔が叩く前に必ずこのスキル該当を確認。トリガー: 「pair-relay」「ペアプロをエージェントで」「ドライバーをエージェントに」「ナビをサブエージェントに」「リレー方式で実装」「司令塔で回して」「navigator と driver を分けて」「長丁場の実装を交代しながら」「レビュー指摘に対応して」「Findings を修正して」「指摘 N 件を直して」「複数 Severe/High を一括修正」など、駆動側もエージェント化したい意図のとき、またはレビュー指摘が複数件あって系統的に修正したいとき。
---

# Pair Relay

司令塔が **Navigator → Driver → Verifier** を **初回 1 回だけ spawn** し、以降は SendMessage で同じ 3 体を回す。

```
User ⇄ 司令塔
        ├─ spawn (初回のみ) → Navigator  (考える・diff を作る)
        ├─ spawn (初回のみ) → Driver     (Edit/Write で diff を貼る)
        └─ spawn (初回のみ) → Verifier   (Read レビュー + cargo check / pytest)
```

## 司令塔の責務

**初回 spawn とメッセージ運搬のみ**。

- ✅ 3 体を最初に spawn、以降は SendMessage で運ぶ
- ✅ subagent から `[respawn-request: <role>]` が返ってきたときのみ再 spawn (引継ぎを原文で貼る)
- ✅ 受け取った出力は次の subagent にそのまま貼って渡す (加工しない)
- ✅ **subagent ↔ User 仲介**: subagent は User と直接話せないので、subagent の方針確認は司令塔が User に聞いて運ぶ
- ✅ User に 1 行進捗報告
- ❌ 上記以外は全て禁止 (それぞれ Driver / Verifier / Verifier / Navigator の責務)

## 役割

| 層 | 担当 |
|---|---|
| **User** | ゴール提示・E2E 確認・最終承認 |
| **司令塔** | spawn 3 体・メッセージ運搬・User 対話 |
| **Navigator** | 次の 1 件 (diff + なぜ) → `.claude/agents/pair-relay-navigator.md` |
| **Driver** | diff を Edit/Write で貼る → `.claude/agents/pair-relay-driver.md` |
| **Verifier** | レビュー + 動作検証 → `.claude/agents/pair-relay-verifier.md` |

## 標準ループ

```
(1) ゴール受領 → 初回 spawn (Navigator / Driver / Verifier 各 1 回)
    ※ Navigator にゴール全体を渡す。Driver/Verifier は待機指示でよい

  繰り返し:
  (3) → Navigator    「次の 1 件((1 ループ = pair-nav の「1 ターン 1 作業」単位))を」          ← diff + なぜ
  (4) → Driver       (Navigator 出力を貼る)   ← "編集完了 (path)"
  (5) → Verifier     (触ったファイルを伝える) ← "✅ pass" or 要約 + 修正 diff
       fail なら (4) へ戻る
  (6) User に 1 行進捗 → 次ステップへ

(7) 全工程完了 → 2〜3 行で総括
```

3 体の context は維持される。毎回ゴールを再送する必要はなく、薄い指示で済む。

## subagent から `[respawn-request: <role>]` が返ってきたとき

context 逼迫した subagent は通常応答の代わりに `[respawn-request: navigator|driver|verifier]` + 引継ぎを返す。司令塔の対応:

1. 同じ subagent_type で **新しい個体を spawn** (旧個体は破棄)
2. 受け取った引継ぎ文章を **初回 spawn prompt にそのまま貼る** (加工しない)
3. 新個体に作業再開を依頼してループに戻る

再 spawn は context 圧迫が解消するための正規ルート。アンチパターンの「再 spawn」は *要求なしの* 再 spawn を指す。

## User から追加レビュー・バグ報告を受けたとき

症状をそのまま Navigator に貼って渡す。司令塔は Read/Grep で推測しない。

## Navigator ↔ User 仲介の例

司令塔は User の言葉を **そのまま** Navigator に流す。要約・整形・「続けて」等の追記もしない。

## やってはいけないこと

| アンチパターン | なぜダメか |
|---|---|
| Navigator / Verifier / Driver の要求なしに再 spawn | context が捨てられ再送コスト |
| 司令塔が Edit/Write | Driver の責務 |
| 司令塔が diff を作る/中身レビュー | Navigator / Verifier の責務 |
| 司令塔が cargo check/pytest を走らす | Verifier の責務 (ログが context に積もる) |
| 司令塔が Read/Grep で調査 | Navigator の責務 |
| 生ログを User に流す | ノイズ。司令塔が消化して 1 行報告 |
