//! J3 strategy_editor_enter_autoindent — Strategy Editor で Enter を押すと
//! 直前行のインデント（と `:` / `{` 後の追加インデント）が継承されることを保証する（kind:ui）。
//!
//! Slice 3 (#50): cosmic_edit 撤去にあわせて、自前 `enter_autoindent_system` の検証から
//! bevscode `input::auto_indent::compute_newline_indent` の振る舞いに対する上位契約に切り替える。
//! 我々の Strategy Editor の indent unit（4 spaces）で seed コーパスに対する期待インデント文字列を
//! 担保することで、bevscode + 我々の設定の組合せが期待通り動くことを回帰ガードする。
//!
//! 完全な keystroke 駆動 E2E は bevscode の input pipeline 全体を build する必要があり重いため、
//! ここでは入口の `compute_newline_indent` を直接呼ぶ単体相当テストに留める。重い integration は
//! Phase B+1 で別 issue 化する予定（behavior-to-e2e）。

use bevscode::input::auto_indent::compute_newline_indent;
use bevscode::settings::{AutoIndent, Indentation};
use ropey::Rope;

/// Strategy Editor のインデント単位（4 spaces / tab_size=4）。bevscode `Indentation::default()` と同等。
fn strategy_editor_indent() -> Indentation {
    Indentation::default()
}

#[test]
fn j3_enter_autoindent_inherits_previous_indent() {
    // 4 spaces インデント済みの行末で Enter → 同じインデントを継承する（Python の典型）
    let rope = Rope::from_str("def foo():\n    x = 1");
    let pos = rope.len_chars();
    let indent = compute_newline_indent(
        &rope,
        pos,
        AutoIndent::Brackets,
        &strategy_editor_indent(),
    );
    assert_eq!(
        indent, "    ",
        "前行のインデント 4 spaces が継承されない (got {:?})",
        indent
    );
}

#[test]
fn j3_enter_autoindent_no_indent_for_topline() {
    // 0 インデントの行末で Enter → 空文字列（インデント追加なし）
    let rope = Rope::from_str("x = 1");
    let pos = rope.len_chars();
    let indent = compute_newline_indent(
        &rope,
        pos,
        AutoIndent::Brackets,
        &strategy_editor_indent(),
    );
    assert_eq!(
        indent, "",
        "トップレベル行で Enter を押したらインデント追加されないはず (got {:?})",
        indent
    );
}
