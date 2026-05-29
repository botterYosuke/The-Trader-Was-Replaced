//! Slice E (#46) — Keyboard drain helper.
//!
//! [`process_key_events`] は純粋関数（テスト可能）。
//! [`drain_keyboard`] は Bevy system から呼ぶラッパー。
//! どちらも `KeyboardDrain` フェーズに属する system から利用する。

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;

/// `process_key_events` / `drain_keyboard` の戻り値。
#[derive(Debug, Default, PartialEq)]
pub struct KeyDrainResult {
    pub enter: bool,
    pub escape: bool,
    pub tab: bool,
    pub backspace_count: u32,
}

/// キーボードイベント列を処理する純粋関数。
///
/// 不変条件:
/// - `Key::Space` → `filter(' ')` が true なら `on_char(' ')` を呼ぶ。
/// - `Key::Escape` → `result.escape = true`、`on_char` は呼ばない。
/// - released イベント（`!ev.state.is_pressed()`）はスキップ。
pub fn process_key_events<'a>(
    events: impl Iterator<Item = &'a KeyboardInput>,
    filter: impl Fn(char) -> bool,
    mut on_char: impl FnMut(char),
) -> KeyDrainResult {
    let mut result = KeyDrainResult::default();
    for ev in events {
        if !ev.state.is_pressed() {
            continue;
        }
        match &ev.logical_key {
            Key::Character(s) => {
                for ch in s.chars() {
                    if filter(ch) {
                        on_char(ch);
                    }
                }
            }
            Key::Space => {
                if filter(' ') {
                    on_char(' ');
                }
            }
            Key::Escape => result.escape = true,
            Key::Enter => result.enter = true,
            Key::Tab => result.tab = true,
            Key::Backspace => result.backspace_count += 1,
            _ => {}
        }
    }
    result
}

/// `Messages<KeyboardInput>` を drain して [`process_key_events`] に渡す system ラッパー。
pub fn drain_keyboard(
    events: &mut ResMut<Messages<KeyboardInput>>,
    filter: impl Fn(char) -> bool,
    on_char: impl FnMut(char),
) -> KeyDrainResult {
    let drained: Vec<KeyboardInput> = events.drain().collect();
    process_key_events(drained.iter(), filter, on_char)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pressed(logical_key: Key) -> KeyboardInput {
        KeyboardInput {
            key_code: bevy::input::keyboard::KeyCode::KeyA,
            logical_key,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        }
    }

    fn make_released(logical_key: Key) -> KeyboardInput {
        KeyboardInput {
            key_code: bevy::input::keyboard::KeyCode::KeyA,
            logical_key,
            state: ButtonState::Released,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        }
    }

    #[test]
    fn released_events_are_skipped() {
        let events = vec![
            make_released(Key::Character("a".into())),
            make_released(Key::Escape),
            make_released(Key::Backspace),
        ];
        let mut chars = Vec::new();
        let result = process_key_events(events.iter(), |_| true, |c| chars.push(c));
        assert_eq!(result, KeyDrainResult::default());
        assert!(chars.is_empty());
    }

    #[test]
    fn char_key_passes_filter() {
        let events = vec![make_pressed(Key::Character("a".into()))];
        let mut chars = Vec::new();
        let result = process_key_events(events.iter(), |_| true, |c| chars.push(c));
        assert_eq!(chars, vec!['a']);
        assert_eq!(result, KeyDrainResult::default());
    }

    #[test]
    fn char_key_blocked_by_filter() {
        let events = vec![
            make_pressed(Key::Character("a".into())),
            make_pressed(Key::Character("1".into())),
        ];
        let mut chars = Vec::new();
        // digit-only filter: 'a' blocked, '1' passes
        let result =
            process_key_events(events.iter(), |c| c.is_ascii_digit(), |c| chars.push(c));
        assert_eq!(chars, vec!['1']);
        assert_eq!(result, KeyDrainResult::default());
    }

    #[test]
    fn space_passes_filter() {
        let events = vec![make_pressed(Key::Space)];
        let mut chars = Vec::new();
        let result = process_key_events(events.iter(), |_| true, |c| chars.push(c));
        assert_eq!(chars, vec![' ']);
        assert_eq!(result, KeyDrainResult::default());
    }

    #[test]
    fn space_blocked_by_filter() {
        let events = vec![make_pressed(Key::Space)];
        let mut chars = Vec::new();
        // filter that rejects space
        let result =
            process_key_events(events.iter(), |c| c.is_alphanumeric(), |c| chars.push(c));
        assert!(chars.is_empty());
        assert_eq!(result, KeyDrainResult::default());
    }

    #[test]
    fn escape_sets_flag_no_on_char() {
        let events = vec![make_pressed(Key::Escape)];
        let mut chars = Vec::new();
        let result = process_key_events(events.iter(), |_| true, |c| chars.push(c));
        assert!(result.escape);
        assert!(chars.is_empty());
    }

    #[test]
    fn enter_sets_flag() {
        let events = vec![make_pressed(Key::Enter)];
        let mut chars = Vec::new();
        let result = process_key_events(events.iter(), |_| true, |c| chars.push(c));
        assert!(result.enter);
        assert!(chars.is_empty());
    }

    #[test]
    fn backspace_increments_count() {
        let events = vec![
            make_pressed(Key::Backspace),
            make_pressed(Key::Backspace),
            make_pressed(Key::Backspace),
        ];
        let mut chars = Vec::new();
        let result = process_key_events(events.iter(), |_| true, |c| chars.push(c));
        assert_eq!(result.backspace_count, 3);
        assert!(chars.is_empty());
    }
}
