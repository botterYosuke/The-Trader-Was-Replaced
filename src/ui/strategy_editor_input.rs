//! Phase C: Tab→spaces / Enter→auto-indent / 括弧の自動補完。
//!
//! いずれも cosmic_edit の既定パスを通らないカスタム編集なので、`CosmicTextChanged` が
//! 自動発火しない (input.rs:518 は `is_edit` のときだけ送る)。**各 system は編集が起きた
//! ときだけ手動で `CosmicTextChanged` を送る** (Caveat #24)。送らないと
//! `sync_editor_to_strategy_buffer_system` に届かず fragment 更新 / undo / autosave /
//! 再ハイライトが空振りする。同一フレーム内に同じ全文を 2 度送っても
//! `fragment.source == new_text` の short-circuit で安全。
//!
//! 実行順:
//! - Tab / Enter: `.before(InputSet)` で cosmic より先に走り、`keys.reset(..)` で
//!   cosmic 側の処理を抑止する (Caveat #3,#4)。
//! - bracket closer: `.after(InputSet)` で cosmic が opener を挿入した直後に closer を
//!   後置する。`Events::clear()` は **呼ばない** (opener 挿入を奪わないため、Caveat #5)。

use crate::ui::strategy_editor::StrategyEditorContent;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy_cosmic_edit::cosmic_text::{self, Action, Edit, Motion};
use bevy_cosmic_edit::prelude::FocusedWidget;
use bevy_cosmic_edit::{CosmicEditor, CosmicFontSystem, CosmicTextChanged};

/// Tab 1 回で挿入する空白数。
const TAB_SPACES: usize = 4;

/// buffer の全行を `\n` 連結して全文を復元する (Caveat #32: `get_text` は fork 内部 trait で
/// 外部から呼べないため代替)。`BufferExtras::get_text` と同一セマンティクス。
fn buffer_text(buffer: &cosmic_text::Buffer) -> String {
    buffer
        .lines
        .iter()
        .map(|l| l.text())
        .collect::<Vec<_>>()
        .join("\n")
}

/// 行頭の連続スペース数 (= 継承すべきインデント幅)。
fn leading_indent_width(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ').count()
}

/// opener 文字に対する closer。`(`/`[`/`{` とクォート 2 種。括弧でなければ None。
fn closer_for(opener: char) -> Option<char> {
    match opener {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' => Some('"'),
        '\'' => Some('\''),
        _ => None,
    }
}

/// closer を後置すべきか。行末 (next None) なら補完する。次の文字が既存の closer 系なら
/// 補完しない (`)` の前で `(` を打っても `))` にしない、の確認に対応)。
fn should_autoclose(_opener: char, next_char: Option<char>) -> bool {
    !matches!(next_char, Some(')' | ']' | '}' | '"' | '\''))
}

/// Ctrl / Super 押下中は cosmic 側が文字挿入をスキップする (`!command` ガード) ので、
/// その間は autoclose も走らせない (opener が入っていないのに closer だけ入る事故防止)。
fn command_modifier_held(keys: &ButtonInput<KeyCode>) -> bool {
    keys.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ])
}

/// Tab → 4 spaces。focused な Strategy Editor のみ。
pub fn tab_input_system(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    focused: Res<FocusedWidget>,
    mut editor_q: Query<(Entity, &mut CosmicEditor), With<StrategyEditorContent>>,
    mut font_system: ResMut<CosmicFontSystem>,
    mut evw_changed: EventWriter<CosmicTextChanged>,
) {
    if !keys.just_pressed(KeyCode::Tab) {
        return;
    }
    let Some(focus_entity) = focused.0 else {
        return;
    };
    let Ok((entity, mut editor)) = editor_q.get_mut(focus_entity) else {
        return;
    };
    for _ in 0..TAB_SPACES {
        editor.action(&mut font_system.0, Action::Insert(' '));
    }
    keys.reset(KeyCode::Tab); // 将来 cosmic が Tab を扱う場合への防衛 + 二重発火防止
    let new_text = editor.with_buffer_mut(|b| buffer_text(b));
    evw_changed.send(CosmicTextChanged((entity, new_text)));
}

/// Enter → 改行 + 前行インデント継承。focused な Strategy Editor のみ。
/// `CosmicWrap::InfiniteLine` 固定なので cursor.line == source 行で、現在行の
/// 行頭スペース数をそのまま新しい行に挿入する。
pub fn enter_autoindent_system(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    focused: Res<FocusedWidget>,
    mut editor_q: Query<(Entity, &mut CosmicEditor), With<StrategyEditorContent>>,
    mut font_system: ResMut<CosmicFontSystem>,
    mut evw_changed: EventWriter<CosmicTextChanged>,
) {
    if !keys.just_pressed(KeyCode::Enter) {
        return;
    }
    let Some(focus_entity) = focused.0 else {
        return;
    };
    let Ok((entity, mut editor)) = editor_q.get_mut(focus_entity) else {
        return;
    };

    // 改行前のカーソル行の行頭インデントを取得する。
    let cursor = editor.cursor();
    let indent = editor.with_buffer(|b| {
        b.lines
            .get(cursor.line)
            .map(|l| leading_indent_width(l.text()))
            .unwrap_or(0)
    });

    editor.action(&mut font_system.0, Action::Insert('\n'));
    for _ in 0..indent {
        editor.action(&mut font_system.0, Action::Insert(' '));
    }
    keys.reset(KeyCode::Enter); // cosmic の Enter 処理を抑止 (Caveat #3)
    let new_text = editor.with_buffer_mut(|b| buffer_text(b));
    evw_changed.send(CosmicTextChanged((entity, new_text)));
}

/// 括弧 opener が入力されたら closer を後置してカーソルを間に残す。
/// cosmic が opener を挿入した直後 (`.after(InputSet)`) に走り、`KeyboardInput` を
/// **読むだけ** (clear しない) で opener 文字を判定する (Caveat #5)。
pub fn bracket_autoclose_system(
    mut keyboard_evr: EventReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    focused: Res<FocusedWidget>,
    mut editor_q: Query<(Entity, &mut CosmicEditor), With<StrategyEditorContent>>,
    mut font_system: ResMut<CosmicFontSystem>,
    mut evw_changed: EventWriter<CosmicTextChanged>,
) {
    // focus が editor 以外なら events を読み捨ててスキップ (溜め込み防止)。
    let Some(focus_entity) = focused.0 else {
        keyboard_evr.clear();
        return;
    };
    let Ok((entity, mut editor)) = editor_q.get_mut(focus_entity) else {
        keyboard_evr.clear();
        return;
    };
    if command_modifier_held(&keys) {
        keyboard_evr.clear();
        return;
    }

    let mut inserted = false;
    for ev in keyboard_evr.read() {
        if ev.state != ButtonState::Pressed {
            continue;
        }
        let Key::Character(s) = &ev.logical_key else {
            continue;
        };
        let Some(opener) = s.chars().next() else {
            continue;
        };
        let Some(closer) = closer_for(opener) else {
            continue;
        };

        // cosmic が opener を挿入した直後のカーソル位置で「次の文字」を見る。
        // cursor.index は byte offset。`get(..)` で範囲外/非境界でも panic しない。
        let cursor = editor.cursor();
        let next_char = editor.with_buffer(|b| {
            b.lines
                .get(cursor.line)
                .and_then(|l| l.text().get(cursor.index..))
                .and_then(|s| s.chars().next())
        });
        if !should_autoclose(opener, next_char) {
            continue;
        }

        editor.action(&mut font_system.0, Action::Insert(closer));
        editor.action(&mut font_system.0, Action::Motion(Motion::Left));
        inserted = true;
    }

    if inserted {
        let new_text = editor.with_buffer_mut(|b| buffer_text(b));
        evw_changed.send(CosmicTextChanged((entity, new_text)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indent_counts_leading_spaces_only() {
        assert_eq!(leading_indent_width("    x"), 4);
        assert_eq!(leading_indent_width("x"), 0);
        assert_eq!(leading_indent_width("  "), 2);
        assert_eq!(leading_indent_width(""), 0);
        // タブは数えない (Tab→spaces なので行頭はスペースのはず)。
        assert_eq!(leading_indent_width("\t  x"), 0);
    }

    #[test]
    fn closer_pairs() {
        assert_eq!(closer_for('('), Some(')'));
        assert_eq!(closer_for('['), Some(']'));
        assert_eq!(closer_for('{'), Some('}'));
        assert_eq!(closer_for('"'), Some('"'));
        assert_eq!(closer_for('\''), Some('\''));
        assert_eq!(closer_for('a'), None);
        assert_eq!(closer_for(')'), None);
    }

    #[test]
    fn autoclose_at_end_of_line() {
        assert!(should_autoclose('(', None));
    }

    #[test]
    fn autoclose_before_plain_char() {
        assert!(should_autoclose('(', Some('a')));
        assert!(should_autoclose('(', Some(' ')));
    }

    #[test]
    fn no_autoclose_before_closer() {
        // `)` の前で `(` を打っても `))` にしない。
        assert!(!should_autoclose('(', Some(')')));
        assert!(!should_autoclose('[', Some(']')));
        assert!(!should_autoclose('"', Some('"')));
    }
}
