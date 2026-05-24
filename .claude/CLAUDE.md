# CLAUDE.md — The-Trader-Was-Replaced

## 実装完了後の必須アクション

実装・修正・フェーズが完了したとき（「完成した」「done」「finished」「実装した」「修正した」「コミットした」「マージする」「フェーズ終了」などのフレーズが出たとき）は、**必ず** `post-impl-skill-update`, `code-review(simplify)` スキルを発動すること。

`post-impl-skill-update`スキルは：
- 今回使用したスキルの振り返り
- 使えばよかった（使い忘れた）スキルの特定
- スキルの description（トリガー条件）や内容の改善

を行い、スキルエコシステムを育てる。

## 機能を追加・変更したときの e2e / wiki メンテナンス（必須）

実装・修正で**機能を追加または変更した**とき（挙動が変わる／新しい不変条件が生まれる／UI 機構を作り替えた など）は、**必ず** `.claude/skills/behavior-to-e2e` スキルを発動し、その機能の **E2E テスト**と **`docs/wiki`** をあわせてメンテナンスすること。具体的には：

- `tests/e2e/FLOWS.md` に対応する flow（例 M12 / N1）を追加・更新する。
- 実装可能なら `tests/e2e_replay.rs` や UI / integration / render harness に自動テスト（回帰ガード）を足す。
- 変更した挙動を `docs/wiki` の該当ページに反映し、本文に対応する `[FlowID]` を引く。wiki が**旧機構を記述したまま実装と食い違っている**場合も現行化する。

「コードだけ直して FLOWS.md / wiki を置き去りにしない」ことが目的。レビュー駆動の修正（codex / Navigator のレビューで挙動を変えた場合）でも同様に発動する。
