//! URL detection and ctrl-click open for `Misc::links`.
//!
//! Two pieces:
//! - `update_link_overlays`: visible-window scanner that produces a
//!   `LinkRects` Component (underline overlays) and a `LinkRanges`
//!   Component (per-URL `(buffer_line, char_start, char_end, url)` for
//!   hit-testing).
//! - `on_ctrl_click_open_url`: a `Pointer<Press>` observer that opens the
//!   URL under the click via the platform's default handler.
//!
//! The scanner is a port of Monaco's `linkComputer.ts` state machine
//! (no regex dep). Recognized schemes: `http://`, `https://`, `file://`.
//! Bracket/paren/curly characters balance against the char preceding
//! the link start, so URLs inside `(…)`, `[…]`, `{…}` round-trip while
//! the wrapping bracket is excluded; quotes terminate the link except
//! when the link itself is quote-wrapped; trailing `.,;:` is stripped.

use bevy::picking::events::{Pointer, Press};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui::ComputedNode;
use bevy_instanced_text::{
    visible_buffer_range, CornerRadii, HiddenLines, MonoCellWidth, RectOverlay, RowVertical,
    TextBounds, TextBuffer,
};
use bevy_instanced_text_editor::RopeBuffer;

use crate::settings::*;
use crate::types::*;

/// One detected URL inside the buffer.
#[derive(Clone, Debug, Reflect)]
#[reflect(Debug)]
pub struct LinkRange {
    /// Buffer line (not display row) the URL starts on.
    pub buffer_line: usize,
    /// Inclusive char offset within the line.
    pub start_char: usize,
    /// Exclusive char offset within the line.
    pub end_char: usize,
    /// The URL itself.
    pub url: String,
}

/// Per-URL hit-test data — written by `update_link_overlays`, read by
/// `on_ctrl_click_open_url`.
#[derive(Component, Default, Clone, Reflect)]
#[reflect(Component, Default)]
pub struct LinkRanges(pub Vec<LinkRange>);

/// Underline overlays per visible URL — written by `update_link_overlays`,
/// merged into `TextOverlays` by `merge_overlay_components`.
#[derive(Component, Default, Clone, Reflect)]
#[reflect(Component, Default)]
pub struct LinkRects(pub Vec<RectOverlay>);

/// Index of the link in [`LinkRanges`] currently under the pointer, or
/// `None` when no link is hovered. Drives the dotted/solid underline
/// swap (hover-only → dotted dim, hover + ctrl → solid bright) and the
/// `Pointer` cursor icon over an active link.
#[derive(Component, Default, Clone, Copy, Reflect)]
#[reflect(Component, Default)]
pub struct HoveredLink(pub Option<usize>);

/// Port of Monaco's `linkComputer.ts` state machine — yields
/// `(start_char, end_char)` pairs for every URL in `line`. Char offsets
/// are 0-based.
///
/// Recognized schemes: `http://`, `https://`, `file://`. Driven by a
/// `(state × char) → state` transition table built once via `OnceLock`.
/// After reaching `Accept`, force-termination characters end the link;
/// trailing punctuation in the `CannotEndIn` class (`.,;:`) is stripped;
/// brackets/parens/braces balanced against the char preceding the link
/// don't terminate, so URLs inside `(…)` or `[…]` round-trip cleanly.
pub fn find_urls(line: &str) -> Vec<(usize, usize)> {
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut out: Vec<(usize, usize)> = Vec::new();

    let mut j = 0usize;
    let mut state = State::Start;
    let mut link_begin: usize = 0;
    let mut link_begin_ch: char = '\0';
    let mut has_open_paren = false;
    let mut has_open_square = false;
    let mut in_square = false;
    let mut has_open_curly = false;

    while j < len {
        let ch = chars[j];
        let mut reset = false;

        if state == State::Accept {
            let class = accept_state_class(
                ch,
                link_begin_ch,
                &mut has_open_paren,
                &mut has_open_square,
                &mut in_square,
                &mut has_open_curly,
            );
            if class == CharClass::ForceTermination {
                push_link(&chars, link_begin, j, &mut out);
                reset = true;
            }
        } else if state == State::End {
            let class = if ch == '[' {
                has_open_square = true;
                CharClass::None
            } else {
                classify(ch)
            };
            if class == CharClass::ForceTermination {
                reset = true;
            } else {
                state = State::Accept;
            }
        } else {
            state = next_state(state, ch);
            if state == State::Invalid {
                reset = true;
            }
        }

        if reset {
            state = State::Start;
            has_open_paren = false;
            has_open_square = false;
            in_square = false;
            has_open_curly = false;
            link_begin = j + 1;
            link_begin_ch = ch;
        }
        j += 1;
    }

    if state == State::Accept {
        push_link(&chars, link_begin, len, &mut out);
    }

    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Invalid,
    Start,
    H,
    HT,
    Htt,
    Http,
    F,
    FI,
    Fil,
    BeforeColon,
    AfterColon,
    AlmostThere,
    End,
    Accept,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CharClass {
    None,
    ForceTermination,
    CannotEndIn,
}

fn next_state(state: State, ch: char) -> State {
    match (state, ch) {
        (State::Start, 'h' | 'H') => State::H,
        (State::Start, 'f' | 'F') => State::F,
        (State::H, 't' | 'T') => State::HT,
        (State::HT, 't' | 'T') => State::Htt,
        (State::Htt, 'p' | 'P') => State::Http,
        (State::Http, 's' | 'S') => State::BeforeColon,
        (State::Http, ':') => State::AfterColon,
        (State::F, 'i' | 'I') => State::FI,
        (State::FI, 'l' | 'L') => State::Fil,
        (State::Fil, 'e' | 'E') => State::BeforeColon,
        (State::BeforeColon, ':') => State::AfterColon,
        (State::AfterColon, '/') => State::AlmostThere,
        (State::AlmostThere, '/') => State::End,
        _ => State::Invalid,
    }
}

/// Classify a character for the post-Accept walk. Mirrors Monaco's
/// `FORCE_TERMINATION_CHARACTERS` + `CANNOT_END_WITH_CHARACTERS` tables
/// (plus the Unicode CJK punctuation Monaco terminates on).
fn classify(ch: char) -> CharClass {
    match ch {
        ' ' | '\t' | '<' | '>' | '\'' | '"' | '`' | '|' | '\u{3001}' | '\u{3002}' | '\u{ff61}'
        | '\u{ff64}' | '\u{ff0c}' | '\u{ff0e}' | '\u{ff1a}' | '\u{ff1b}' | '\u{2018}'
        | '\u{3008}' | '\u{300c}' | '\u{300e}' | '\u{3014}' | '\u{ff08}' | '\u{ff3b}'
        | '\u{ff5b}' | '\u{ff62}' | '\u{ff63}' | '\u{ff5d}' | '\u{ff3d}' | '\u{ff09}'
        | '\u{3015}' | '\u{300f}' | '\u{300d}' | '\u{3009}' | '\u{2019}' | '\u{ff40}'
        | '\u{ff5e}' | '\u{2026}' => CharClass::ForceTermination,
        '.' | ',' | ';' | ':' => CharClass::CannotEndIn,
        _ => CharClass::None,
    }
}

/// Per-character class lookup while in `Accept`, with bracket/quote
/// balancing state. Updates the in/out bracket flags inline since their
/// transitions are tied to which character we're classifying.
fn accept_state_class(
    ch: char,
    link_begin_ch: char,
    has_open_paren: &mut bool,
    has_open_square: &mut bool,
    in_square: &mut bool,
    has_open_curly: &mut bool,
) -> CharClass {
    match ch {
        '(' => {
            *has_open_paren = true;
            CharClass::None
        }
        ')' => {
            if *has_open_paren {
                CharClass::None
            } else {
                CharClass::ForceTermination
            }
        }
        '[' => {
            *in_square = true;
            *has_open_square = true;
            CharClass::None
        }
        ']' => {
            *in_square = false;
            if *has_open_square {
                CharClass::None
            } else {
                CharClass::ForceTermination
            }
        }
        '{' => {
            *has_open_curly = true;
            CharClass::None
        }
        '}' => {
            if *has_open_curly {
                CharClass::None
            } else {
                CharClass::ForceTermination
            }
        }
        '\'' | '"' | '`' => {
            if link_begin_ch == ch {
                CharClass::ForceTermination
            } else if matches!(link_begin_ch, '\'' | '"' | '`') {
                CharClass::None
            } else {
                CharClass::ForceTermination
            }
        }
        '*' => {
            if link_begin_ch == '*' {
                CharClass::ForceTermination
            } else {
                CharClass::None
            }
        }
        ' ' => {
            if *in_square {
                CharClass::None
            } else {
                CharClass::ForceTermination
            }
        }
        _ => classify(ch),
    }
}

/// Emit a link covering `chars[begin..end]` after trimming
/// `CannotEndIn` trailing punctuation and shrinking by one when the link
/// is wrapped in a balanced bracket whose closer immediately follows.
fn push_link(chars: &[char], begin: usize, end: usize, out: &mut Vec<(usize, usize)>) {
    let mut last_included = end.saturating_sub(1);
    while last_included > begin && classify(chars[last_included]) == CharClass::CannotEndIn {
        last_included -= 1;
    }
    if begin > 0 && last_included > begin {
        let before = chars[begin - 1];
        let last = chars[last_included];
        let wraps = matches!((before, last), ('(', ')') | ('[', ']') | ('{', '}'));
        if wraps {
            last_included -= 1;
        }
    }
    let final_end = last_included + 1;
    if final_end > begin {
        out.push((begin, final_end));
    }
}

/// Build per-URL underline overlays + hit-test ranges for the visible
/// window. Mirrors `update_indent_guides` / `update_rulers` in shape.
///
/// Three visual states, matching Monaco:
/// - **Idle** (no hover): no overlay.
/// - **Hover, no ctrl/cmd**: dotted dim underline drawn as multiple short
///   `Underline` segments (engine has no native dotted variant).
/// - **Hover + ctrl/cmd**: solid bright underline (the active state, also
///   the clickable state — observer fires on click).
///
/// `LinkRanges` is populated regardless of hover so the hover observer
/// and the click observer have hit-test data.
pub(crate) fn update_link_overlays(
    mut editor_query: Query<
        (
            EditorRenderView,
            Option<&HiddenLines>,
            Option<&TextBounds>,
            &EditorTheme,
            &Misc,
            &HoveredLink,
            &mut LinkRects,
            &mut LinkRanges,
        ),
        With<CodeEditor>,
    >,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    let ctrl_held = keyboard.pressed(KeyCode::ControlLeft)
        || keyboard.pressed(KeyCode::ControlRight)
        || keyboard.pressed(KeyCode::SuperLeft)
        || keyboard.pressed(KeyCode::SuperRight);

    for (
        rv, hidden, bounds, theme, misc, hovered, mut link_rects, mut link_ranges,
    ) in editor_query.iter_mut()
    {
        if !misc.links {
            if !link_rects.0.is_empty() {
                link_rects.0.clear();
            }
            if !link_ranges.0.is_empty() {
                link_ranges.0.clear();
            }
            continue;
        }

        let m = rv.metrics();
        let wrap_cfg = bounds.copied().unwrap_or_default();
        let visible = visible_buffer_range(
            &**rv.buffer,
            rv.scroll.y,
            m.viewport_height,
            m.text_area_top,
            m.line_height,
            m.char_width,
            wrap_cfg,
            hidden,
        );

        let mut new_rects: Vec<RectOverlay> = Vec::new();
        let mut new_ranges: Vec<LinkRange> = Vec::new();
        let dim = with_alpha(theme.link, 0.45);

        if visible.start < visible.end {
            for buffer_line in visible.start..visible.end {
                if rv.fold.is_line_hidden(buffer_line) {
                    continue;
                }
                let line = rv.buffer.line(buffer_line);
                let line_text = line.to_string();
                let matches = find_urls(&line_text);
                if matches.is_empty() {
                    continue;
                }
                for (start_char, end_char) in matches {
                    let url: String = line_text
                        .chars()
                        .skip(start_char)
                        .take(end_char - start_char)
                        .collect();
                    let this_idx = new_ranges.len();
                    new_ranges.push(LinkRange {
                        buffer_line,
                        start_char,
                        end_char,
                        url,
                    });

                    let is_hovered = hovered.0 == Some(this_idx);
                    if !is_hovered {
                        continue;
                    }

                    let s_byte = line.slice(..start_char).len_bytes();
                    let e_byte = line.slice(..end_char).len_bytes();

                    let (start_row, start_byte_in_row) = rv
                        .layout
                        .and_then(|l| l.buffer_to_display(buffer_line as u32, s_byte))
                        .unwrap_or_else(|| {
                            (rv.fold.actual_to_display_line(buffer_line) as u32, s_byte)
                        });
                    let (end_row, end_byte_in_row) = rv
                        .layout
                        .and_then(|l| l.buffer_to_display(buffer_line as u32, e_byte))
                        .unwrap_or_else(|| {
                            (rv.fold.actual_to_display_line(buffer_line) as u32, e_byte)
                        });
                    let start_x = rv
                        .layout
                        .and_then(|l| l.x_at_byte(start_row, start_byte_in_row))
                        .unwrap_or(start_char as f32 * m.char_width);
                    let end_x = if start_row == end_row {
                        rv
                            .layout
                            .and_then(|l| {
                                l.x_after_source_range(end_row, start_byte_in_row, end_byte_in_row)
                            })
                            .unwrap_or(end_char as f32 * m.char_width)
                    } else {
                        rv
                            .layout
                            .and_then(|l| l.x_at_byte(end_row, end_byte_in_row))
                            .unwrap_or(end_char as f32 * m.char_width)
                    };

                    let push =
                        |out: &mut Vec<RectOverlay>, row: u32, range: std::ops::Range<f32>| {
                            if ctrl_held {
                                out.push(underline_rect(row, range, theme.link));
                            } else {
                                push_dotted_underline(out, row, range, dim, m.char_width);
                            }
                        };

                    if start_row == end_row {
                        push(&mut new_rects, start_row, start_x..end_x);
                    } else {
                        let start_row_end = rv
                            .layout
                            .and_then(|l| {
                                l.lines
                                    .iter()
                                    .find(|line| line.display_row == start_row)
                                    .and_then(|line| l.x_at_byte(start_row, line.text.len()))
                            })
                            .unwrap_or(end_char as f32 * m.char_width);
                        push(&mut new_rects, start_row, start_x..start_row_end);
                        for r in (start_row + 1)..end_row {
                            push(&mut new_rects, r, 0.0..start_row_end);
                        }
                        push(&mut new_rects, end_row, 0.0..end_x);
                    }
                }
            }
        }

        if link_rects.0 != new_rects {
            link_rects.0 = new_rects;
        }
        if !range_lists_equal(&link_ranges.0, &new_ranges) {
            link_ranges.0 = new_ranges;
        }
    }
}

fn with_alpha(c: Color, a: f32) -> Color {
    let s = c.to_srgba();
    Color::srgba(s.red, s.green, s.blue, a)
}

/// Fake a dotted underline by emitting short `Underline` segments along
/// `range`. Each segment is ~⅓ of a character cell with an equal gap, so
/// the dash period scales naturally with the font size.
fn push_dotted_underline(
    out: &mut Vec<RectOverlay>,
    row: u32,
    range: std::ops::Range<f32>,
    color: Color,
    char_width: f32,
) {
    let dash = (char_width * 0.30).max(1.5);
    let gap = (char_width * 0.30).max(1.5);
    let mut x = range.start;
    while x < range.end {
        let seg_end = (x + dash).min(range.end);
        out.push(underline_rect(row, x..seg_end, color));
        x = seg_end + gap;
    }
}

/// Mouse-move observer: track which link index in [`LinkRanges`] (if
/// any) the pointer is currently over, writing it to [`HoveredLink`].
/// The producer reads this every frame to swap underline styling, and
/// `sync_cursor_icon` reads it (together with the Ctrl/Cmd modifier) to
/// flip to a pointer cursor.
pub fn on_pointer_move_for_link_hover(
    trigger: On<bevy::picking::events::Pointer<bevy::picking::events::Move>>,
    mut editor_query: Query<
        (
            &TextBuffer<RopeBuffer>,
            &ComputedNode,
            &bevy_instanced_text::DisplayLayout,
            &MonoCellWidth,
            &LinkRanges,
            &Misc,
            &mut HoveredLink,
        ),
        With<CodeEditor>,
    >,
) {
    let entity = trigger.event().entity;
    let Ok((buffer, computed, layout, mono, link_ranges, misc, mut hovered)) =
        editor_query.get_mut(entity)
    else {
        return;
    };
    if !misc.links {
        if hovered.0.is_some() {
            hovered.0 = None;
        }
        return;
    }
    let Some(local_pos) = crate::input::mouse::hit_to_local_px(&trigger.event().hit, computed)
    else {
        if hovered.0.is_some() {
            hovered.0 = None;
        }
        return;
    };
    let inv = computed.inverse_scale_factor();
    let text_area_left = computed.content_inset().min_inset.x * inv;
    let relative_x = local_pos.x - text_area_left;
    let Some(buffer_line) =
        crate::plugin::gutter_decorations::buffer_line_at_y(layout, local_pos.y)
    else {
        if hovered.0.is_some() {
            hovered.0 = None;
        }
        return;
    };
    let rope = buffer.rope();
    if buffer_line >= rope.len_lines() {
        if hovered.0.is_some() {
            hovered.0 = None;
        }
        return;
    }
    let line = rope.line(buffer_line);
    let col = (relative_x / mono.px).max(0.0) as usize;
    let col = col.min(line.len_chars().saturating_sub(1));

    let next = link_ranges
        .0
        .iter()
        .position(|r| r.buffer_line == buffer_line && col >= r.start_char && col < r.end_char);
    if hovered.0 != next {
        hovered.0 = next;
    }
}

fn range_lists_equal(a: &[LinkRange], b: &[LinkRange]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b.iter()).all(|(x, y)| {
            x.buffer_line == y.buffer_line
                && x.start_char == y.start_char
                && x.end_char == y.end_char
                && x.url == y.url
        })
}

fn underline_rect(display_row: u32, x_range: std::ops::Range<f32>, color: Color) -> RectOverlay {
    RectOverlay {
        display_row,
        x_range,
        vertical: RowVertical::Underline {
            thickness: 1.0,
            gap: 3.0,
        },
        color,
        z: 0,
        corners: CornerRadii::ZERO,
    }
}

/// Ctrl+click observer: when the click lands on a detected URL, open it
/// with the platform's default handler and short-circuit any other
/// ctrl-click behavior. Registered before `on_ctrl_click_goto_definition`
/// so the URL path wins when both could match — and `goto_definition`
/// itself checks `LinkRanges` to back off when a URL covers the click.
pub fn on_ctrl_click_open_url(
    trigger: On<Pointer<Press>>,
    editor_query: Query<
        (
            &TextBuffer<RopeBuffer>,
            &ComputedNode,
            &bevy_instanced_text::DisplayLayout,
            &MonoCellWidth,
            &LinkRanges,
            &Misc,
        ),
        With<CodeEditor>,
    >,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    if trigger.event().button != PointerButton::Primary {
        return;
    }
    if !(keyboard.pressed(KeyCode::ControlLeft)
        || keyboard.pressed(KeyCode::ControlRight)
        || keyboard.pressed(KeyCode::SuperLeft)
        || keyboard.pressed(KeyCode::SuperRight))
    {
        return;
    }
    let entity = trigger.event().entity;
    let Ok((buffer, computed, layout, mono, link_ranges, misc)) = editor_query.get(entity) else {
        return;
    };
    if !misc.links {
        return;
    }
    let Some(local_pos) = crate::input::mouse::hit_to_local_px(&trigger.event().hit, computed)
    else {
        return;
    };

    let inv = computed.inverse_scale_factor();
    let text_area_left = computed.content_inset().min_inset.x * inv;
    let relative_x = local_pos.x - text_area_left;
    let Some(buffer_line) =
        crate::plugin::gutter_decorations::buffer_line_at_y(layout, local_pos.y)
    else {
        return;
    };

    let line = buffer.rope().line(buffer_line);
    let col = (relative_x / mono.px).max(0.0) as usize;
    let col = col.min(line.len_chars().saturating_sub(1));

    let Some(hit) = link_ranges
        .0
        .iter()
        .find(|r| r.buffer_line == buffer_line && col >= r.start_char && col < r.end_char)
    else {
        return;
    };
    open_url(&hit.url);
}

/// Open a URL via the platform default handler. Failures are swallowed —
/// URL launching is best-effort and we don't want a missing handler to
/// propagate as a panic.
fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let cmd = ("open", &[url]);
    #[cfg(target_os = "windows")]
    let cmd = ("cmd", &["/C", "start", "", url]);
    #[cfg(all(unix, not(target_os = "macos")))]
    let cmd = ("xdg-open", &[url]);

    let _ = std::process::Command::new(cmd.0).args(cmd.1).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slice(line: &str, range: (usize, usize)) -> String {
        line.chars().skip(range.0).take(range.1 - range.0).collect()
    }

    #[test]
    fn finds_http_url() {
        let line = "see http://example.com for info";
        let r = find_urls(line);
        assert_eq!(r.len(), 1);
        assert_eq!(slice(line, r[0]), "http://example.com");
    }

    #[test]
    fn finds_https_url() {
        let line = "https://example.com/path?x=1";
        let r = find_urls(line);
        assert_eq!(r.len(), 1);
        assert_eq!(slice(line, r[0]), "https://example.com/path?x=1");
    }

    #[test]
    fn strips_trailing_punctuation() {
        let line = "visit https://example.com.";
        let r = find_urls(line);
        assert_eq!(r.len(), 1);
        assert_eq!(slice(line, r[0]), "https://example.com");
    }

    #[test]
    fn ignores_bare_text() {
        let r = find_urls("no urls here even if i mention example.com");
        assert!(r.is_empty());
    }

    #[test]
    fn finds_multiple_urls() {
        let r = find_urls("a http://a.com b http://b.com");
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn balanced_parens_kept_inside() {
        let line = "see https://en.wikipedia.org/wiki/Rust_(programming_language) ok";
        let r = find_urls(line);
        assert_eq!(r.len(), 1);
        assert_eq!(
            slice(line, r[0]),
            "https://en.wikipedia.org/wiki/Rust_(programming_language)"
        );
    }

    #[test]
    fn wrapping_paren_excluded() {
        let line = "see (https://example.com) ok";
        let r = find_urls(line);
        assert_eq!(r.len(), 1);
        assert_eq!(slice(line, r[0]), "https://example.com");
    }

    #[test]
    fn finds_file_url() {
        let line = "file:///etc/hosts";
        let r = find_urls(line);
        assert_eq!(r.len(), 1);
        assert_eq!(slice(line, r[0]), "file:///etc/hosts");
    }
}
