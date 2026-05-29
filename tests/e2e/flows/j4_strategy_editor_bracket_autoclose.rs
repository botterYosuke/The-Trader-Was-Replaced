//! J4 strategy_editor_bracket_autoclose — Strategy Editor で `(` / `[` / `{` / `"` / `'` を入力すると
//! 対応する閉じ括弧 / クオートを補完することを保証する（kind:ui）。
//!
//! Slice 3 (#50): cosmic_edit 撤去にあわせて、自前 `bracket_autoclose_system` の検証から
//! bevscode が提供する `input::actions::get_closing_bracket` / `get_closing_quote` の上位契約に
//! 切り替える。Strategy Editor で実際に使う opener セットに対して正しい closer が返ることを担保し、
//! bevscode + 我々の設定の組合せが期待通りに括弧補完を行うことを回帰ガードする。
//!
//! 完全な keystroke 駆動 E2E は bevscode の input pipeline 全体を build する必要があり重いため、
//! ここでは入口の純関数を直接呼ぶ単体相当テストに留める。重い integration は Phase B+1 で
//! 別 issue 化する予定（behavior-to-e2e）。

use bevscode::input::actions::{get_closing_bracket, get_closing_quote};

/// Strategy Editor が想定する bracket ペア（Python / Rust 共通の基本セット）。
/// bevscode 既定とほぼ同等。
const BRACKET_PAIRS: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}')];

#[test]
fn j4_bracket_autoclose_returns_matching_closer() {
    assert_eq!(get_closing_bracket('(', BRACKET_PAIRS), Some(')'));
    assert_eq!(get_closing_bracket('[', BRACKET_PAIRS), Some(']'));
    assert_eq!(get_closing_bracket('{', BRACKET_PAIRS), Some('}'));
}

#[test]
fn j4_bracket_autoclose_returns_none_for_non_opener() {
    assert_eq!(get_closing_bracket(')', BRACKET_PAIRS), None);
    assert_eq!(get_closing_bracket('x', BRACKET_PAIRS), None);
    assert_eq!(get_closing_bracket(' ', BRACKET_PAIRS), None);
}

#[test]
fn j4_quote_autoclose_returns_matching_quote() {
    assert_eq!(get_closing_quote('"'), Some('"'));
    assert_eq!(get_closing_quote('\''), Some('\''));
}

#[test]
fn j4_quote_autoclose_returns_none_for_non_quote() {
    assert_eq!(get_closing_quote('('), None);
    assert_eq!(get_closing_quote('a'), None);
}
