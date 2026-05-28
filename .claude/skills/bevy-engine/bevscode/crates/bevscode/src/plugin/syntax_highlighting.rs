//! Editor-side syntax highlighting glue for tree-sitter.

use crate::text_view::TextBuffer;
use crate::types::CodeEditor;
use crate::types::LineSegment;
use bevy::prelude::*;
use bevy_instanced_text_editor::RopeBuffer;

use std::sync::{Arc, RwLock};

type InitEditorSyntaxQuery<'w, 's> = Query<
    'w,
    's,
    (Entity, Option<&'static bevy_tree_sitter::TreeSitterGrammar>),
    (With<CodeEditor>, Without<EditorSyntaxState>),
>;

type ReactLanguageChangedQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static bevy_tree_sitter::TreeSitterGrammar,
        &'static mut EditorSyntaxState,
    ),
    (
        With<CodeEditor>,
        Changed<bevy_tree_sitter::TreeSitterGrammar>,
    ),
>;

type SyncEditorParseSourceQuery<'w, 's> = Query<
    'w,
    's,
    (
        Ref<'static, TextBuffer<RopeBuffer>>,
        &'static EditorParseBufferRef,
    ),
    With<CodeEditor>,
>;

#[derive(Component, Default)]
pub struct EditorSyntaxState {
    pub(crate) provider: Option<bevy_tree_sitter::TreeSitterProvider>,
    /// Buffer-line range covered by the last `produce_line_styles` pass.
    /// Tracked here (not on `LineStyles`) because it's producer state, not
    /// renderer input — the engine never reads it.
    pub(crate) covered: std::ops::Range<u32>,
}

impl EditorSyntaxState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_provider(&mut self, provider: bevy_tree_sitter::TreeSitterProvider) {
        self.provider = Some(provider);
    }

    pub fn is_available(&self) -> bool {
        self.provider
            .as_ref()
            .map(|p| p.is_available())
            .unwrap_or(false)
    }

    /// True when `byte_offset` is somewhere a completion request makes
    /// sense — i.e. *not* inside a string literal or comment. Callers pass
    /// the tree from `SyntaxTree` directly; returns `true` when absent.
    pub fn is_completion_context(tree: &bevy_tree_sitter::ts::Tree, byte_offset: usize) -> bool {
        matches!(syntax_context(tree, byte_offset), SyntaxContext::Other)
    }
}

/// Coarse tree-sitter context bucket — drives Monaco-style `quickSuggestions`
/// per-context toggles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxContext {
    Other,
    Comment,
    String,
}

/// Classify the cursor's context for `quickSuggestions`: comment, string, or
/// "other" (code).
pub fn syntax_context(tree: &bevy_tree_sitter::ts::Tree, byte_offset: usize) -> SyntaxContext {
    let root = tree.root_node();
    if byte_offset > root.end_byte() {
        return SyntaxContext::Other;
    }
    let Some(node) = root.descendant_for_byte_range(byte_offset, byte_offset) else {
        return SyntaxContext::Other;
    };
    let mut cur = Some(node);
    while let Some(n) = cur {
        let kind = n.kind();
        if kind.contains("comment") {
            return SyntaxContext::Comment;
        }
        if kind.contains("string") || kind == "raw_string_literal" {
            return SyntaxContext::String;
        }
        cur = n.parent();
    }
    SyntaxContext::Other
}

impl EditorSyntaxState {
    /// Highlight a sequence of buffer lines, each tagged with its absolute
    /// rope byte offset. Returns one styled-segment vector per input line.
    ///
    /// Passing per-line rope offsets (rather than a single joined block with
    /// cumulative offsets) is what lets the styler stay correct across
    /// hidden / folded lines: each visible line maps to its true position
    /// in the rope, so tree-sitter highlight ranges align even when the
    /// caller skipped buffer rows in between.
    pub fn highlight_lines(
        &mut self,
        lines: &[(usize, &str)],
        syntax_tree: &bevy_tree_sitter::SyntaxTree,
        rope: &ropey::Rope,
        theme: &crate::settings::SyntaxColors,
        default_color: Color,
    ) -> Vec<Vec<LineSegment>> {
        let Some((first_byte, last_byte)) = lines.first().zip(lines.last()).map(|(f, l)| {
            let f_byte = f.0;
            let l_byte = l.0 + l.1.len();
            (f_byte, l_byte)
        }) else {
            return Vec::new();
        };
        let Some(provider) = &mut self.provider else {
            return plain_lines(lines, default_color);
        };
        let Some(tree) = syntax_tree.tree.as_ref() else {
            return plain_lines(lines, default_color);
        };
        match provider.highlight_range(tree, rope, first_byte..last_byte) {
            Some(highlights) => lines_to_segments(lines, &highlights, theme, default_color),
            None => plain_lines(lines, default_color),
        }
    }
}

/// Plain-text fallback for `highlight_lines` when no tree-sitter provider
/// is available. One segment per non-blank line in `lines`.
fn plain_lines(lines: &[(usize, &str)], default_color: Color) -> Vec<Vec<LineSegment>> {
    lines
        .iter()
        .map(|(_, line)| {
            let stripped = line.strip_suffix('\n').unwrap_or(line);
            if stripped.trim().is_empty() {
                vec![]
            } else {
                vec![LineSegment {
                    text: stripped.to_string(),
                    color: default_color,
                    background: None,
                    corner_radius: 0.0,
                    font_scale: 0.0,
                    skew: 0.0,
                }]
            }
        })
        .collect()
}

/// Translate a flat sorted `HighlightRange` slice into per-line `LineSegment`s,
/// mapping capture names through the editor's `SyntaxColors`.
///
/// Each `lines` entry is `(absolute_rope_byte_start, line_text)`. Lines may
/// be non-contiguous in the rope (the caller can skip rows hidden by folds)
/// as long as each one carries its true rope offset.
///
/// `highlights` is document-absolute and sorted by `byte_range.start`.
/// Two-pointer walk: O(sum(line_len) + H) where H = highlight count.
fn lines_to_segments(
    lines: &[(usize, &str)],
    highlights: &[bevy_tree_sitter::HighlightRange],
    theme: &crate::settings::SyntaxColors,
    default_color: Color,
) -> Vec<Vec<LineSegment>> {
    let mut out: Vec<Vec<LineSegment>> = Vec::with_capacity(lines.len());
    let mut hi_idx = 0usize;

    for (abs_line_start, raw_line) in lines.iter().copied() {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let line_len = line.len();
        let abs_line_end = abs_line_start + line_len;

        while hi_idx < highlights.len() && highlights[hi_idx].byte_range.end <= abs_line_start {
            hi_idx += 1;
        }

        let mut segments: Vec<LineSegment> = Vec::new();
        let mut cursor = abs_line_start;
        let mut local_hi = hi_idx;

        while cursor < abs_line_end {
            while local_hi < highlights.len() && highlights[local_hi].byte_range.end <= cursor {
                local_hi += 1;
            }

            if local_hi < highlights.len() {
                let hl = &highlights[local_hi];
                let hl_start = hl.byte_range.start.max(abs_line_start);
                let hl_end = hl.byte_range.end.min(abs_line_end);

                if hl_start > cursor {
                    let lo = line.floor_char_boundary((cursor - abs_line_start).min(line.len()));
                    let hi = line.floor_char_boundary((hl_start - abs_line_start).min(line.len()));
                    let slice = &line[lo..hi];
                    if !slice.is_empty() {
                        segments.push(LineSegment {
                            text: slice.to_string(),
                            color: default_color,
                            background: None,
                            corner_radius: 0.0,
                            font_scale: 0.0,
                            skew: 0.0,
                        });
                    }
                    cursor = hl_start;
                } else {
                    let lo = line.floor_char_boundary((cursor - abs_line_start).min(line.len()));
                    let hi = line.floor_char_boundary((hl_end - abs_line_start).min(line.len()));
                    let slice = &line[lo..hi];
                    if !slice.is_empty() {
                        let color = crate::syntax::map_highlight_color(
                            Some(&hl.capture_name),
                            theme,
                            default_color,
                        );
                        segments.push(LineSegment {
                            text: slice.to_string(),
                            color,
                            background: None,
                            corner_radius: 0.0,
                            font_scale: 0.0,
                            skew: 0.0,
                        });
                    }
                    cursor = hl_end;
                    local_hi += 1;
                }
            } else {
                let lo = line.floor_char_boundary((cursor - abs_line_start).min(line.len()));
                let slice = &line[lo..];
                if !slice.is_empty() {
                    segments.push(LineSegment {
                        text: slice.to_string(),
                        color: default_color,
                        background: None,
                        corner_radius: 0.0,
                        font_scale: 0.0,
                        skew: 0.0,
                    });
                }
                cursor = abs_line_end;
            }
        }

        if segments.iter().all(|s| s.text.trim().is_empty()) {
            out.push(Vec::new());
        } else {
            out.push(segments);
        }
    }

    out
}

#[derive(Default)]
pub(crate) struct EditorBufferSnapshot {
    pub(crate) rope: ropey::Rope,
    pub(crate) content_version: u64,
}

pub(crate) struct EditorParseSource {
    pub(crate) buf: Arc<RwLock<EditorBufferSnapshot>>,
}

impl bevy_tree_sitter::ParseSource for EditorParseSource {
    fn content_version(&self) -> u64 {
        self.buf.read().unwrap().content_version
    }

    fn snapshot(&self) -> ropey::Rope {
        self.buf.read().unwrap().rope.clone()
    }
}

pub fn init_editor_syntax(mut commands: Commands, editors: InitEditorSyntaxQuery) {
    for (entity, grammar) in editors.iter() {
        let mut syntax_state = EditorSyntaxState::new();

        if let Some(g) = grammar {
            if let Some(provider) = g.create_provider() {
                syntax_state.provider = Some(provider);
            }
        }

        let buf = Arc::new(RwLock::new(EditorBufferSnapshot::default()));
        let parse_source = EditorParseSource { buf: buf.clone() };

        commands.entity(entity).insert((
            syntax_state,
            EditorParseBufferRef(buf),
            bevy_tree_sitter::ParseSourceComp::new(parse_source),
            bevy_tree_sitter::SyntaxTree::default(),
        ));
    }
}

#[derive(Component)]
pub(crate) struct EditorParseBufferRef(pub(crate) Arc<RwLock<EditorBufferSnapshot>>);

pub(crate) fn react_language_changed(mut editors: ReactLanguageChangedQuery) {
    for (grammar, mut syntax_state) in editors.iter_mut() {
        if let Some(provider) = grammar.create_provider() {
            syntax_state.provider = Some(provider);
        }
    }
}

pub(crate) fn sync_editor_parse_source(editors: SyncEditorParseSourceQuery) {
    for (buffer, buf_ref) in editors.iter() {
        // `is_changed()` can miss the first observation of a buffer when
        // `EditorParseBufferRef` is attached on a later tick than the buffer
        // itself (e.g. host-side `SetTextRequested` flows). Fall back to
        // comparing rope byte length so we still resync after such a load.
        let buffer_bytes = buffer.rope().len_bytes();
        let needs_sync = buffer.is_changed() || {
            let snap = buf_ref.0.read().unwrap();
            snap.rope.len_bytes() != buffer_bytes
        };
        if !needs_sync {
            continue;
        }
        let mut buf = buf_ref.0.write().unwrap();
        buf.rope = buffer.rope().clone();
        buf.content_version = buf.content_version.wrapping_add(1);
    }
}

pub(crate) fn record_edits_for_incremental_parsing(
    mut editor_query: Query<&mut bevy_tree_sitter::SyntaxTree, With<CodeEditor>>,
    mut events: MessageReader<crate::types::events::TextEdited>,
) {
    let collected_events: Vec<_> = events.read().cloned().collect();
    for mut syntax_tree in editor_query.iter_mut() {
        for event in collected_events.iter() {
            let d = &event.delta;
            let edit = bevy_tree_sitter::ts::InputEdit {
                start_byte: d.start_byte,
                old_end_byte: d.old_end_byte,
                new_end_byte: d.new_end_byte,
                start_position: bevy_tree_sitter::ts::Point::new(
                    d.start_position.row as usize,
                    d.start_position.column_byte as usize,
                ),
                old_end_position: bevy_tree_sitter::ts::Point::new(
                    d.old_end_position.row as usize,
                    d.old_end_position.column_byte as usize,
                ),
                new_end_position: bevy_tree_sitter::ts::Point::new(
                    d.new_end_position.row as usize,
                    d.new_end_position.column_byte as usize,
                ),
            };

            let removed = edit.old_end_byte.saturating_sub(edit.start_byte);
            let inserted = edit.new_end_byte.saturating_sub(edit.start_byte);
            const HUGE_EDIT_THRESHOLD: usize = 64 * 1024;
            let huge_edit = removed > HUGE_EDIT_THRESHOLD || inserted > HUGE_EDIT_THRESHOLD;

            if !huge_edit {
                let st = syntax_tree.bypass_change_detection();
                if let Some(tree) = st.tree.as_mut() {
                    tree.edit(&edit);
                }
                let start_row = edit.start_position.row as u32;
                let end_row = edit.new_end_position.row as u32;
                st.dirty_rows = Some(match st.dirty_rows {
                    Some((lo, hi)) => (lo.min(start_row), hi.max(end_row)),
                    None => (start_row, end_row),
                });
            } else {
                let st = syntax_tree.bypass_change_detection();
                st.tree = None;
                st.dirty_rows = None;
            }
        }
    }
}

pub struct SyntaxPlugin;

impl Plugin for SyntaxPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<crate::types::events::TextEdited>();

        app.add_systems(Startup, init_editor_syntax);
        app.add_systems(
            Update,
            init_editor_syntax.in_set(crate::plugin::ApplyStateSet),
        );

        {
            if !app.is_plugin_added::<bevy_tree_sitter::TreeSitterPlugin>() {
                app.add_plugins(bevy_tree_sitter::TreeSitterPlugin);
            }

            app.add_systems(
                Update,
                (
                    react_language_changed,
                    sync_editor_parse_source,
                    record_edits_for_incremental_parsing,
                )
                    .chain()
                    .in_set(crate::plugin::ApplyStateSet)
                    .before(bevy_tree_sitter::ParseSet),
            );
        }
    }
}
