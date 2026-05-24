# CLAUDE.md — The-Trader-Was-Replaced

## 実装完了後の必須アクション

実装・修正・フェーズが完了したとき（「完成した」「done」「finished」「実装した」「修正した」「コミットした」「マージする」「フェーズ終了」などのフレーズが出たとき）は、**必ず** `post-impl-skill-update`, `code-review(simplify)` スキルを発動すること。

### `post-impl-skill-update`スキルは：
- 今回使用したスキルの振り返り
- 使えばよかった（使い忘れた）スキルの特定
- スキルの description（トリガー条件）や内容の改善

を行い、スキルエコシステムを育てる。

### `code-review(simplify)` スキルを発動した際は：
Navigator と セカンドオピニオンとして `codex` にレビューを依頼して修正すること：

```
codex exec -s read-only "claude -p --permission-mode acceptEdits"
```

**Medium** 以上の指摘が無くなるまで `/pair-relay` でレビュー＆修正を繰り返すこと。

## 不具合の修正をする前の E2E メンテナンス（必須）

不具合を修正するときは、**先に RED の E2E テストを作成してから**コードを直すこと。順序は以下の通り：

1. **RED を書く** — バグを再現する `#[test]` を `tests/e2e/flows/` に追加し `tests/e2e_replay.rs` に登録する。
   `behavior-to-e2e` スキルを使って FLOWS.md に flow を追記する。
2. **RED を確認する** — `cargo test --test e2e_replay <id>` を実行し、**assert で fail する**ことを確認する
   （compile error や panic ではなく、意図した assert で落ちること）。
3. **コードを修正する** — バグを直す。
4. **GREEN を確認する** — 同じテストが pass することを確認する。
5. **全体を通す** — `cargo test --test e2e_replay` で他テストへの回帰がないことを確認する。

### なぜこの順序か

- RED を先に書くことで「このテストが本当にバグを検知できるか」を確かめられる。
- GREEN になってから「そのテストが fix を検知していただけか、最初から pass するだけか」が区別できる。
- 修正後に追加したテストは「fix を確認するテスト」にしかならず、バグが再発しても検知できないことがある。

### FLOWS.md への記載

RED のまま登録した flow は FLOWS.md に `- [ ]`（未チェック）かつ以下のコメントを付ける：

```
RED＝回帰ガード・fix は #issue 後に green
```

fix 後は `- [x]` に更新し、コメントを削除する。

## 機能を追加・変更したときの e2e / wiki メンテナンス（必須）

実装・修正で**機能を追加または変更した**とき（挙動が変わる／新しい不変条件が生まれる／UI 機構を作り替えた など）は、**必ず** `.claude/skills/behavior-to-e2e` スキルを発動し、その機能の **E2E テスト**と **`docs/wiki`** をあわせてメンテナンスすること。具体的には：

- `tests/e2e/FLOWS.md` に対応する flow（例 M12 / N1）を追加・更新する。
- 実装可能なら `tests/e2e_replay.rs` や UI / integration / render harness に自動テスト（回帰ガード）を足す。
- 変更した挙動を `docs/wiki` の該当ページに反映し、本文に対応する `[FlowID]` を引く。wiki が**旧機構を記述したまま実装と食い違っている**場合も現行化する。

「コードだけ直して FLOWS.md / wiki を置き去りにしない」ことが目的。レビュー駆動の修正（codex / Navigator のレビューで挙動を変えた場合）でも同様に発動する。
