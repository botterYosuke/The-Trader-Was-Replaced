---
name: tdd
description: Test-driven development with red-green-refactor loop. Use when user wants to build features or fix bugs using TDD, mentions "red-green-refactor", wants integration tests, or asks for test-first development. ALSO use when a task spec / issue / instruction mandates TDD or "write a failing regression test first" (e.g. "TDD 厳守", "RED→GREEN", "回帰テストを先に書いて失敗を確認してから実装", "must write the regression test from RED") — the trigger is the instruction requiring test-first, not only the user phrasing it live. Especially for safety-critical bug fixes where a RED test must prove the bug before the fix. ALSO use when an issue says "these existing tests currently assert the bug as intended behavior and must be **inverted** RED→GREEN" — invert the assertion first (make it RED), confirm failure, then fix the implementation (GREEN). This pattern is common in this repo's issue workflow: "test_xxx_rejected → 成功ケースに反転", "テストを反転", "既存テストを成功ケースに変更". ALSO covers the stale-test sub-case: a test left behind by a *prior* shipped feature is **already RED** (failing) because production already moved to the new behavior — here inverting the test to the current spec IS the whole fix, with NO production-code change (e.g. "#30 でホームモード化したのに test が旧 precondition を assert したまま RED"). Confirm RED, invert to current behavior, confirm GREEN, run the suite for regressions. Fire even when the fix looks like a trivial one-line assertion flip, and fire when an issue body explicitly names the `tdd` skill / its RED→GREEN inversion pattern.
---

# Test-Driven Development

## Philosophy

**Core principle**: Tests should verify behavior through public interfaces, not implementation details. Code can change entirely; tests shouldn't.

**Good tests** are integration-style: they exercise real code paths through public APIs. They describe _what_ the system does, not _how_ it does it. A good test reads like a specification - "user can checkout with valid cart" tells you exactly what capability exists. These tests survive refactors because they don't care about internal structure.

**Bad tests** are coupled to implementation. They mock internal collaborators, test private methods, or verify through external means (like querying a database directly instead of using the interface). The warning sign: your test breaks when you refactor, but behavior hasn't changed. If you rename an internal function and tests fail, those tests were testing implementation, not behavior.

See [tests.md](tests.md) for examples and [mocking.md](mocking.md) for mocking guidelines.

## Anti-Pattern: Horizontal Slices

**DO NOT write all tests first, then all implementation.** This is "horizontal slicing" - treating RED as "write all tests" and GREEN as "write all code."

This produces **crap tests**:

- Tests written in bulk test _imagined_ behavior, not _actual_ behavior
- You end up testing the _shape_ of things (data structures, function signatures) rather than user-facing behavior
- Tests become insensitive to real changes - they pass when behavior breaks, fail when behavior is fine
- You outrun your headlights, committing to test structure before understanding the implementation

**Correct approach**: Vertical slices via tracer bullets. One test → one implementation → repeat. Each test responds to what you learned from the previous cycle. Because you just wrote the code, you know exactly what behavior matters and how to verify it.

```
WRONG (horizontal):
  RED:   test1, test2, test3, test4, test5
  GREEN: impl1, impl2, impl3, impl4, impl5

RIGHT (vertical):
  RED→GREEN: test1→impl1
  RED→GREEN: test2→impl2
  RED→GREEN: test3→impl3
  ...
```

## Workflow

### 1. Planning

When exploring the codebase, use the project's domain glossary so that test names and interface vocabulary match the project's language, and respect ADRs in the area you're touching.

Before writing any code:

- [ ] Confirm with user what interface changes are needed
- [ ] Confirm with user which behaviors to test (prioritize)
- [ ] Identify opportunities for [deep modules](deep-modules.md) (small interface, deep implementation)
- [ ] Design interfaces for [testability](interface-design.md)
- [ ] List the behaviors to test (not implementation steps)
- [ ] Get user approval on the plan

Ask: "What should the public interface look like? Which behaviors are most important to test?"

**You can't test everything.** Confirm with the user exactly which behaviors matter most. Focus testing effort on critical paths and complex logic, not every possible edge case.

### 2. Tracer Bullet

Write ONE test that confirms ONE thing about the system:

```
RED:   Write test for first behavior → test fails
GREEN: Write minimal code to pass → test passes
```

This is your tracer bullet - proves the path works end-to-end.

> **必ず RED を「実行して」確認してから実装に入る。** 仕様/issue が「回帰テストは RED 済み」と書いていても鵜呑みにしない。まず `cargo test <名前>` を走らせる。**もし既に GREEN なら、その修正は既にコミット済みの可能性が高い**（issue が OPEN なまま放置されているだけ）。`git log --oneline -- <対象ファイル>` と該当 system/guard の実装を grep で当て直し、本当に未実装か確認する。既に実装済みなら**再実装せず**、検証結果を添えてユーザーに報告する（二重実装・既存コードの上書きを防ぐ）。例: issue #23 は「i15 RED 済み」と書かれていたが実際は HEAD コミットで A+B 解決済み・i15 green で、着手前の RED 実行で発覚した。
>
> **自分の「最初の Read」も鵜呑みにしない。** セッション中に `git pull`/merge で HEAD が動き、序盤に Read したファイルが古くなることがある（作業ツリーが裏で更新される）。RED を書く直前に `git log --oneline -3` で HEAD を確認し、対象ファイルは Grep で当て直してから着手する。例: issue #24 は着手時の Read で supervisor に `LIVE_VENUE` 配線が皆無に見えたが、その後の merge（`05e5c491`）で spawn 側配線が既に入っており、Grep で再確認して初めて「残作業は attach 側照合のみ」と判明した。Read 1 回ぶんの古いスナップショットで設計すると、既存実装を再実装・上書きしかける。
>
> **このリポジトリの watcher 制約 → 「build-green / runtime-RED」で RED を作る。** この workspace は外部 watcher が走っており、**`cargo build --lib` がコンパイル不可な間は未コミットの編集（テスト含む）を巻き戻す**（bevy-engine skill 参照）。そのため「未定義シンボルを参照して compile error を出す」古典的 RED は、書いた瞬間に watcher に巻き戻されて消える。回避策＝**RED テストは lib が「コンパイルできる」形で書き、実行時に落とす**：既存シンボルだけを参照し、挙動は `app.update()` 等で駆動して assert を外す（watcher は red **build** でのみ巻き戻す。red **test** は安全）。RED の確認は「ビルドは通った／テストは runtime で FAILED（例: `left: Inherited, right: Hidden`）」で行う。**例外**: enum バリアント新規追加など、exhaustive match を壊して compile error が不可避な土台ステップは、最小の GREEN（バリアント＋全 match arm）を同じ手にまとめて lib を green に保つ（テストは intent を記録する characterization 寄りになる）。例: issue #25 は全スライスをこの build-green/runtime-RED で回し、各手を `cargo build --lib` green に保ったまま TDD した。

### 3. Incremental Loop

For each remaining behavior:

```
RED:   Write next test → fails
GREEN: Minimal code to pass → passes
```

Rules:

- One test at a time
- Only enough code to pass current test
- Don't anticipate future tests
- Keep tests focused on observable behavior

### 4. Refactor

After all tests pass, look for [refactor candidates](refactoring.md):

- [ ] Extract duplication
- [ ] Deepen modules (move complexity behind simple interfaces)
- [ ] Apply SOLID principles where natural
- [ ] Consider what new code reveals about existing code
- [ ] Run tests after each refactor step

**Never refactor while RED.** Get to GREEN first.

## Checklist Per Cycle

```
[ ] Test describes behavior, not implementation
[ ] Test uses public interface only
[ ] Test would survive internal refactor
[ ] Code is minimal for this test
[ ] No speculative features added
```
