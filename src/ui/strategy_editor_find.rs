//! Phase 7.2 Phase E — Find / Replace パネル。
//!
//! 設計原則:
//! - Find マッチの **計算と描画は分離**: 計算は `compute_find_match_spans_system` が
//!   `FindReplaceState.matches` (ナビ用) と editor entity の `FindMatchSpans` (描画用) を
//!   同時に更新するだけ。色付けは Phase A の composer (`apply_highlight_layers_system`)
//!   が固定順序で行う。本モジュールは `set_attrs_list` を **絶対に呼ばない**。
//! - Find パネルの 2 つの入力欄には専用マーカー (`FindQueryEditor` / `FindReplacementEditor`)
//!   を付け、`StrategyEditorContent` は **絶対に付けない** (Caveat #21)。これにより
//!   既存 `sync_editor_to_strategy_buffer_system` が Find 入力を Strategy 本文に書き込む
//!   事故を構造的に防ぐ。
//! - Replace は純粋関数 `apply_replacement` で新ソースを計算し、editor へ `set_text` +
//!   `CosmicTextChanged` を発行するだけ。再ハイライト / undo 記録 / autosave は Phase A
//!   パイプライン (Changed<StrategyFragment> 駆動) に丸投げする (新しい attrs 経路を作らない)。

use crate::ui::components::{StrategyEditorId, StrategyFragment, WindowRoot};
use crate::ui::floating_window::{FloatingWindowSpec, spawn_floating_window};
use crate::ui::strategy_editor::StrategyEditorContent;
use crate::ui::strategy_editor_highlight::{FindMatchSpans, MatchSpan};
use bevy::prelude::*;
use bevy_cosmic_edit::cosmic_text::{self, Attrs, AttrsOwned, Edit, Metrics, Shaping};
use bevy_cosmic_edit::prelude::{
    CosmicColor, CosmicEditBuffer, CosmicEditor, DefaultAttrs, FocusedWidget, TextEdit2d,
};
use bevy_cosmic_edit::{
    CosmicBackgroundColor, CosmicFontSystem, CosmicTextAlign, CosmicTextChanged, CursorColor,
    MaxLines,
};

// ── レイアウト定数 ──────────────────────────────────────────────
const FIND_PANEL_SIZE: Vec2 = Vec2::new(440.0, 210.0);
const FIND_PANEL_POSITION: Vec2 = Vec2::new(260.0, 160.0);
const FIND_ACCENT: Color = Color::srgba(1.0, 0.7, 0.2, 0.4); // orange rim
const FIND_FONT_SIZE: f32 = 14.0;
const FIND_LINE_HEIGHT: f32 = 18.0;
// ⚠️ 高さは line_height(18) の DPI 2x ダブリング(=36)を超える必要がある。
// 24px だと retina で `shape_until_scroll` が layout_runs=0 を返し glyph が出ない
// (bevy-engine skill の DPI トラップ)。44px で 1x/2x 両対応。
const FIELD_SIZE: Vec2 = Vec2::new(300.0, 44.0);
const FIELD_BG: Color = Color::srgba(0.05, 0.05, 0.08, 1.0);

// ─────────────────────────────────────────────────────────────────
// 状態 / マーカー / イベント
// ─────────────────────────────────────────────────────────────────

/// Find/Replace のグローバル状態 (multi-spawn でも単一: 最後に focus した editor が対象)。
#[derive(Resource, Default)]
pub struct FindReplaceState {
    pub query: String,
    pub replacement: String,
    pub case_sensitive: bool,
    /// ナビゲーション用マッチ列 (描画用は editor entity の `FindMatchSpans`)。
    pub matches: Vec<MatchSpan>,
    pub current: usize,
    pub is_open: bool,
    pub target_editor: Option<Entity>,
    pub panel_root: Option<Entity>,
    pub query_editor: Option<Entity>,
    pub replacement_editor: Option<Entity>,
}

/// Find パネルの query 入力欄マーカー (StrategyEditorContent は付けない)。
#[derive(Component)]
pub struct FindQueryEditor;

/// Find パネルの replacement 入力欄マーカー (StrategyEditorContent は付けない)。
#[derive(Component)]
pub struct FindReplacementEditor;

/// マッチ件数表示用 Text2d マーカー。
#[derive(Component)]
pub struct FindMatchCountText;

/// Find パネルのボタン種別 (entity に貼って observer から引く)。
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum FindButtonKind {
    Prev,
    Next,
    Replace,
    ReplaceAll,
}

/// ボタン click / キーボードから発行されるアクション要求。
#[derive(Event, Clone, Copy)]
pub struct FindActionRequested(pub FindButtonKind);

// ─────────────────────────────────────────────────────────────────
// 純粋関数 (ユニットテスト対象)
// ─────────────────────────────────────────────────────────────────

/// `source` を行単位に走査し、`query` の **重なりなし** マッチをすべて返す。
/// 各マッチは (行 index, 行内 byte range)。空クエリは空 Vec。
/// `case_sensitive=false` は文字単位で小文字化して比較し、消費 byte 長は
/// haystack 側の元バイト数で数えるので元ソースの byte offset と整合する。
pub fn find_matches(source: &str, query: &str, case_sensitive: bool) -> Vec<MatchSpan> {
    if query.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (line_idx, line) in source.lines().enumerate() {
        let mut blocked_until = 0usize;
        for (b, _) in line.char_indices() {
            if b < blocked_until {
                continue;
            }
            if let Some(consumed) = match_len_at(&line[b..], query, case_sensitive) {
                out.push(MatchSpan {
                    line: line_idx,
                    byte_range: b..b + consumed,
                });
                blocked_until = b + consumed;
            }
        }
    }
    out
}

/// `haystack` が先頭で `needle` にマッチするなら、haystack 側で消費した byte 数を返す。
/// case-insensitive のときは文字単位で `to_lowercase` 比較する。
fn match_len_at(haystack: &str, needle: &str, case_sensitive: bool) -> Option<usize> {
    if case_sensitive {
        return haystack.starts_with(needle).then_some(needle.len());
    }
    let mut hay = haystack.chars();
    let mut consumed = 0usize;
    for nc in needle.chars() {
        match hay.next() {
            Some(hc) if hc.to_lowercase().eq(nc.to_lowercase()) => consumed += hc.len_utf8(),
            _ => return None,
        }
    }
    Some(consumed)
}

/// Replace のスコープ。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReplaceScope {
    /// `matches[current]` のみ置換。
    Current,
    /// すべてのマッチを置換。
    All,
}

/// `matches` の (行, 行内 byte range) を絶対 byte offset に変換し、`replacement` で置換した
/// 新しいソースを返す。置換は **絶対 offset の降順** (右→左) に適用するので、前方の range が
/// 後方の置換でズレない。`matches` が空なら `source` をそのまま返す。
pub fn apply_replacement(
    source: &str,
    matches: &[MatchSpan],
    current: usize,
    replacement: &str,
    scope: ReplaceScope,
) -> String {
    if matches.is_empty() {
        return source.to_string();
    }
    let line_starts = line_start_offsets(source);
    let mut ranges: Vec<std::ops::Range<usize>> = match scope {
        ReplaceScope::All => matches
            .iter()
            .filter_map(|m| abs_range(&line_starts, m))
            .collect(),
        ReplaceScope::Current => matches
            .get(current)
            .and_then(|m| abs_range(&line_starts, m))
            .into_iter()
            .collect(),
    };
    // 右→左に適用 (start 降順)。
    ranges.sort_by_key(|r| std::cmp::Reverse(r.start));
    let mut out = source.to_string();
    for r in ranges {
        if r.end <= out.len() {
            out.replace_range(r, replacement);
        }
    }
    out
}

/// 各行の先頭の絶対 byte offset。`find_matches` の行 index と整合する
/// (`source.lines()` と同じく `\n` 区切り)。
fn line_start_offsets(source: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

/// (行, 行内 range) を絶対 byte range に変換。
fn abs_range(line_starts: &[usize], m: &MatchSpan) -> Option<std::ops::Range<usize>> {
    let &start = line_starts.get(m.line)?;
    Some(start + m.byte_range.start..start + m.byte_range.end)
}

// ─────────────────────────────────────────────────────────────────
// システム
// ─────────────────────────────────────────────────────────────────

/// Ctrl+F で開く / Esc で閉じる。`target_editor` には押下時に focus 中の Strategy editor を保存する。
pub fn find_keyboard_system(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut cooldown: Local<f32>,
    mut state: ResMut<FindReplaceState>,
    mut focused: ResMut<FocusedWidget>,
    editor_q: Query<(), With<StrategyEditorContent>>,
) {
    *cooldown = (*cooldown - time.delta_secs()).max(0.0);

    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if ctrl && keys.just_pressed(KeyCode::KeyF) && *cooldown <= 0.0 {
        if !state.is_open {
            state.is_open = true;
            // focus 中が Strategy editor ならそれを対象に。そうでなければ None
            // (パネルは開くが、対象が無ければマッチ計算は no-op)。
            state.target_editor = focused.0.filter(|e| editor_q.contains(*e));
        } else {
            // 既に開いている: 別の Strategy editor に focus 中ならそれへ retarget
            // (旧 target のハイライトは compute_find_match_spans_system が target 切替検知で clear)。
            // query 欄に focus 中 (= 通常) は filter→None で retarget せず、query 欄へ戻すだけ。
            if let Some(new_target) = focused.0.filter(|e| editor_q.contains(*e)) {
                state.target_editor = Some(new_target);
            }
            if let Some(qe) = state.query_editor {
                focused.0 = Some(qe);
            }
        }
        *cooldown = 0.5;
    }

    if keys.just_pressed(KeyCode::Escape) && state.is_open {
        state.is_open = false;
        if let Some(target) = state.target_editor {
            focused.0 = Some(target);
        }
    }
}

/// `is_open` の false↔true 遷移で Find パネルを 1 度だけ spawn / despawn する (Caveat #27)。
/// 親パネルが外部から despawn された場合 (× ボタン等) は孤児チェックで state をリセットする。
pub fn manage_find_panel_lifecycle_system(
    mut commands: Commands,
    mut state: ResMut<FindReplaceState>,
    mut font_system: ResMut<CosmicFontSystem>,
    mut focused: ResMut<FocusedWidget>,
    // 孤児チェックは `With<WindowRoot>` を使う (Caveat #31)。panel_root は WindowRoot を持つ。
    existence_q: Query<(), With<WindowRoot>>,
) {
    // 孤児チェック: panel_root が外から消えていたら state をリセット (= 開き直し可能に)。
    if let Some(root) = state.panel_root
        && existence_q.get(root).is_err()
    {
        let restore = state.target_editor;
        *state = FindReplaceState::default();
        if let Some(t) = restore {
            focused.0 = Some(t);
        }
        return;
    }

    // open 遷移: spawn。
    if state.is_open && state.panel_root.is_none() {
        let (root, content, _title) = spawn_floating_window(
            &mut commands,
            FloatingWindowSpec {
                title: "FIND / REPLACE".to_string(),
                size: FIND_PANEL_SIZE,
                position: FIND_PANEL_POSITION,
                accent: FIND_ACCENT,
            },
        );

        // 入力欄 2 つ。
        let query_editor = spawn_find_field(
            &mut commands,
            &mut font_system,
            FIELD_SIZE,
            Vec3::new(15.0, 50.0, 0.2),
        );
        commands.entity(query_editor).insert(FindQueryEditor);
        commands.entity(content).add_child(query_editor);

        let replacement_editor = spawn_find_field(
            &mut commands,
            &mut font_system,
            FIELD_SIZE,
            Vec3::new(15.0, 0.0, 0.2),
        );
        commands
            .entity(replacement_editor)
            .insert(FindReplacementEditor);
        commands.entity(content).add_child(replacement_editor);

        // ラベル。
        spawn_label(&mut commands, content, "Find", Vec3::new(-205.0, 50.0, 0.2));
        spawn_label(&mut commands, content, "Repl", Vec3::new(-205.0, 0.0, 0.2));

        // 件数表示。
        let count = commands
            .spawn((
                Text2d::new("0/0"),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(Color::srgb(0.7, 0.7, 0.75)),
                bevy::sprite::Anchor::CenterLeft,
                Transform::from_xyz(-205.0, -55.0, 0.2),
                FindMatchCountText,
            ))
            .id();
        commands.entity(content).add_child(count);

        // ボタン (Prev / Next / Replace / Replace All)。
        spawn_button(
            &mut commands,
            content,
            FindButtonKind::Prev,
            "<",
            Vec3::new(-120.0, -55.0, 0.2),
            Vec2::new(36.0, 24.0),
        );
        spawn_button(
            &mut commands,
            content,
            FindButtonKind::Next,
            ">",
            Vec3::new(-78.0, -55.0, 0.2),
            Vec2::new(36.0, 24.0),
        );
        spawn_button(
            &mut commands,
            content,
            FindButtonKind::Replace,
            "Repl",
            Vec3::new(-10.0, -55.0, 0.2),
            Vec2::new(56.0, 24.0),
        );
        spawn_button(
            &mut commands,
            content,
            FindButtonKind::ReplaceAll,
            "Repl All",
            Vec3::new(80.0, -55.0, 0.2),
            Vec2::new(90.0, 24.0),
        );

        state.panel_root = Some(root);
        state.query_editor = Some(query_editor);
        state.replacement_editor = Some(replacement_editor);
        // query 欄にフォーカスを移す (open 直後 1 度だけ)。
        focused.0 = Some(query_editor);
    }

    // close 遷移: despawn。
    if !state.is_open && state.panel_root.is_some() {
        let root = state.panel_root.take().unwrap();
        commands.entity(root).despawn_recursive();
        state.query_editor = None;
        state.replacement_editor = None;
        // matches のクリアは compute_find_match_spans_system が is_open=false で行う。
    }
}

/// Find 入力欄 (query / replacement) の `CosmicTextChanged` を `FindReplaceState` に書き戻す。
/// **history / autosave / fragment には絶対に触らない** (Caveat #28)。
/// `Without<StrategyEditorContent>` を二重ガードに使い、マーカー取り違えを検出する。
pub fn sync_find_editors_to_state_system(
    mut events: EventReader<CosmicTextChanged>,
    query_q: Query<Entity, (With<FindQueryEditor>, Without<StrategyEditorContent>)>,
    replacement_q: Query<Entity, (With<FindReplacementEditor>, Without<StrategyEditorContent>)>,
    mut state: ResMut<FindReplaceState>,
) {
    for CosmicTextChanged((entity, new_text)) in events.read() {
        if query_q.contains(*entity) {
            if state.query != *new_text {
                state.query = new_text.clone();
            }
        } else if replacement_q.contains(*entity) && state.replacement != *new_text {
            state.replacement = new_text.clone();
        }
    }
}

/// `FindReplaceState` または対象 fragment の変化でマッチを再計算し、
/// `state.matches` (ナビ用) と target editor の `FindMatchSpans` (描画用) を同時更新する。
/// `set_attrs_list` は呼ばない — 色付けは composer の責務 (Caveat #8)。
///
/// `last` Local は前回 compute 時の入力 `(query, case_sensitive, is_open, target)`。
/// (名前付き struct にすると pub システムの signature に private 型が露出するためタプルで持つ。)
pub fn compute_find_match_spans_system(
    mut state: ResMut<FindReplaceState>,
    changed_frags: Query<(), (With<WindowRoot>, Changed<StrategyFragment>)>,
    fragments_q: Query<(&StrategyEditorId, &StrategyFragment), With<WindowRoot>>,
    mut editor_q: Query<(&StrategyEditorId, &mut FindMatchSpans), With<StrategyEditorContent>>,
    mut last: Local<(String, bool, bool, Option<Entity>)>,
) {
    // 孤児チェック: target_editor が despawn 済み (× で閉じられた等) なら state をリセット。
    // ⚠️ `*state = default()` だけだと panel_root の Entity 参照が失われ、Find パネル本体
    // (別 entity) が despawn されずに画面へ残留する (リーク)。state は default に戻しつつ
    // パネル系ハンドルだけ残し、is_open=false → manage_find_panel_lifecycle_system の
    // close 遷移に despawn を委ねる。
    if let Some(target) = state.target_editor
        && editor_q.get(target).is_err()
    {
        let panel_root = state.panel_root;
        let query_editor = state.query_editor;
        let replacement_editor = state.replacement_editor;
        *state = FindReplaceState::default();
        state.panel_root = panel_root;
        state.query_editor = query_editor;
        state.replacement_editor = replacement_editor;
        *last = Default::default();
        return;
    }

    // ⚠️ 再計算トリガは「マッチを左右する入力 (query / case / open / target / source)」の変化だけで
    // 判定する。`state.is_changed()` で代用すると find_navigate_system の `current` 変更でも発火し、
    // 翌フレームに current=0 へ戻してナビゲーションが死ぬ (ResMut の DerefMut は値に関係なく
    // Resource 全体を changed にするため)。さらに compute 自身が毎フレーム state.matches を書くので
    // is_changed() は open 中ずっと true のままになり、settle しない。
    let (last_query, last_case, last_open, last_target) = &*last;
    let query_changed = *last_query != state.query || *last_case != state.case_sensitive;
    let open_changed = *last_open != state.is_open;
    let target_changed = *last_target != state.target_editor;
    let source_changed = !changed_frags.is_empty();
    if !query_changed && !open_changed && !target_changed && !source_changed {
        return;
    }

    let prev_target = last.3;
    *last = (
        state.query.clone(),
        state.case_sensitive,
        state.is_open,
        state.target_editor,
    );

    // target 切替時は旧 target のハイライトを消す (multi-spawn で残らないように)。
    if target_changed
        && let Some(old) = prev_target
        && Some(old) != state.target_editor
        && let Ok((_, mut old_spans)) = editor_q.get_mut(old)
    {
        old_spans.prev_match_lines = old_spans.matches.iter().map(|m| m.line).collect();
        old_spans.matches = Vec::new();
        old_spans.current_idx = None;
    }

    let Some(target) = state.target_editor else {
        return;
    };
    let Ok((target_id, mut spans)) = editor_q.get_mut(target) else {
        return;
    };
    let region_key = target_id.region_key.clone();

    // 旧マッチ行を保存 (クリア時に composer の dirty 行へ含めるため、上書き前に)。
    spans.prev_match_lines = spans.matches.iter().map(|m| m.line).collect();

    // 閉じている / クエリ空 → マッチをクリアして終了。
    if !state.is_open || state.query.is_empty() {
        state.matches = Vec::new();
        state.current = 0;
        spans.matches = Vec::new();
        spans.current_idx = None;
        return;
    }

    let Some(source) = fragments_q
        .iter()
        .find(|(id, _)| id.region_key == region_key)
        .map(|(_, f)| f.source.clone())
    else {
        return;
    };

    let new_matches = find_matches(&source, &state.query, state.case_sensitive);
    let n = new_matches.len();

    // query/case が変わった or target が切り替わった → 先頭マッチへ。
    // source だけ変化 → current 保持 (編集中もナビ位置を維持)。いずれもマッチ数でクランプ。
    if query_changed || target_changed {
        state.current = 0;
    }
    state.current = if n == 0 { 0 } else { state.current.min(n - 1) };

    spans.matches = new_matches.clone();
    spans.current_idx = (n > 0).then_some(state.current);
    state.matches = new_matches;
}

/// Enter / F3 (Shift で逆方向) と Next/Prev ボタンで現在マッチを移動する。
/// query 欄に focus 中は Enter を navigation に使わない (F3 / ボタンのみ)。
pub fn find_navigate_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut actions: EventReader<FindActionRequested>,
    focused: Res<FocusedWidget>,
    mut state: ResMut<FindReplaceState>,
    mut editor_q: Query<&mut FindMatchSpans, With<StrategyEditorContent>>,
) {
    if !state.is_open || state.matches.is_empty() {
        return;
    }

    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let f3 = keys.just_pressed(KeyCode::F3);
    let enter = keys.just_pressed(KeyCode::Enter);
    let query_focused = state.query_editor.is_some_and(|qe| focused.0 == Some(qe));
    // ⚠️ 対象エディタに focus 中の Enter は enter_autoindent_system が改行として処理する。
    // ここでも navigation に使うと「改行 + 次マッチ移動」の二重発火になる (両 system 間に
    // 順序指定が無く非決定的)。target editor に focus 中は Enter-nav を無効化し、F3 のみで移動する。
    let target_focused = state.target_editor.is_some_and(|t| focused.0 == Some(t));
    let enter_nav = enter && !query_focused && !target_focused;

    let mut forward = (f3 && !shift) || (enter_nav && !shift);
    let mut backward = (f3 && shift) || (enter_nav && shift);
    for a in actions.read() {
        match a.0 {
            FindButtonKind::Next => forward = true,
            FindButtonKind::Prev => backward = true,
            _ => {}
        }
    }
    if !forward && !backward {
        return;
    }

    let n = state.matches.len();
    state.current = if forward {
        (state.current + 1) % n
    } else {
        (state.current + n - 1) % n
    };
    let cur = state.current;
    if let Some(target) = state.target_editor
        && let Ok(mut spans) = editor_q.get_mut(target)
    {
        spans.current_idx = Some(cur);
    }
}

/// `current_idx` が変わったら対象行を画面中央付近へスクロールする。
#[allow(clippy::type_complexity)]
pub fn find_scroll_to_match_system(
    state: Res<FindReplaceState>,
    mut editor_q: Query<
        (
            Option<&mut CosmicEditor>,
            &mut CosmicEditBuffer,
            &FindMatchSpans,
        ),
        (With<StrategyEditorContent>, Changed<FindMatchSpans>),
    >,
) {
    if !state.is_open {
        return;
    }
    for (editor_opt, mut buffer, spans) in editor_q.iter_mut() {
        let Some(current_idx) = spans.current_idx else {
            continue;
        };
        let Some(m) = spans.matches.get(current_idx) else {
            continue;
        };
        let viewport_lines = 16usize; // 粗い近似で十分。
        let target_line = m.line.saturating_sub(viewport_lines / 2);
        let scroll = cosmic_text::Scroll {
            line: target_line,
            vertical: 0.0,
            horizontal: 0.0,
        };
        if let Some(mut editor) = editor_opt {
            editor.with_buffer_mut(|b| b.set_scroll(scroll));
            editor.set_redraw(true);
        } else {
            buffer.0.set_scroll(scroll);
            buffer.0.set_redraw(true);
        }
    }
}

/// Replace / Replace All ボタンで `apply_replacement` を実行し、対象 editor へ
/// `set_text` + `CosmicTextChanged` を発行する。fragment 更新・undo 記録・autosave・
/// 再ハイライトは Phase A パイプライン (CosmicTextChanged → fragment Changed) に委譲する。
pub fn replace_execute_system(
    mut actions: EventReader<FindActionRequested>,
    state: Res<FindReplaceState>,
    fragments_q: Query<(&StrategyEditorId, &StrategyFragment), With<WindowRoot>>,
    mut editor_q: Query<
        (
            &StrategyEditorId,
            &mut CosmicEditBuffer,
            Option<&mut CosmicEditor>,
        ),
        With<StrategyEditorContent>,
    >,
    mut font_system: ResMut<CosmicFontSystem>,
    mut evw_changed: EventWriter<CosmicTextChanged>,
) {
    if !state.is_open {
        return;
    }
    let mut scope: Option<ReplaceScope> = None;
    for a in actions.read() {
        match a.0 {
            FindButtonKind::Replace => scope = Some(ReplaceScope::Current),
            FindButtonKind::ReplaceAll => scope = Some(ReplaceScope::All),
            _ => {}
        }
    }
    let Some(scope) = scope else {
        return;
    };
    if state.matches.is_empty() {
        return;
    }
    let Some(target) = state.target_editor else {
        return;
    };
    let Ok((editor_id, mut edit_buffer, editor_opt)) = editor_q.get_mut(target) else {
        return;
    };
    let region_key = editor_id.region_key.clone();
    let Some(source) = fragments_q
        .iter()
        .find(|(id, _)| id.region_key == region_key)
        .map(|(_, f)| f.source.clone())
    else {
        return;
    };

    let new_source = apply_replacement(
        &source,
        &state.matches,
        state.current,
        &state.replacement,
        scope,
    );
    if new_source == source {
        return;
    }

    // 表示バッファを更新し、CosmicTextChanged で fragment/undo/autosave/再ハイライトを駆動する。
    edit_buffer.set_text(&mut font_system, &new_source, Attrs::new());
    if let Some(mut editor) = editor_opt {
        editor.with_buffer_mut(|b| {
            b.set_text(
                &mut font_system.0,
                &new_source,
                Attrs::new(),
                Shaping::Advanced,
            );
            b.set_redraw(true);
        });
        editor.set_redraw(true);
    }
    evw_changed.send(CosmicTextChanged((target, new_source)));
}

/// 件数表示 ("current/total") を state 変化時に更新する。
pub fn update_find_count_text_system(
    state: Res<FindReplaceState>,
    mut q: Query<&mut Text2d, With<FindMatchCountText>>,
) {
    if !state.is_changed() {
        return;
    }
    let label = if state.matches.is_empty() {
        "0/0".to_string()
    } else {
        format!("{}/{}", state.current + 1, state.matches.len())
    };
    for mut t in q.iter_mut() {
        if t.0 != label {
            t.0 = label.clone();
        }
    }
}

// ─────────────────────────────────────────────────────────────────
// spawn ヘルパ
// ─────────────────────────────────────────────────────────────────

/// Find パネル内の 1 行入力欄 (cosmic editor) を spawn する。役割マーカーは呼び出し側で insert。
fn spawn_find_field(
    commands: &mut Commands,
    font_system: &mut CosmicFontSystem,
    size: Vec2,
    pos: Vec3,
) -> Entity {
    let text_color = CosmicColor::rgb(230, 230, 230);
    commands
        .spawn((
            TextEdit2d,
            Sprite {
                custom_size: Some(size),
                color: Color::WHITE,
                ..default()
            },
            CosmicEditBuffer::new(
                &mut font_system.0,
                Metrics::new(FIND_FONT_SIZE, FIND_LINE_HEIGHT),
            )
            .with_text(&mut font_system.0, "", Attrs::new().color(text_color)),
            DefaultAttrs(AttrsOwned::new(Attrs::new().color(text_color))),
            CursorColor(Color::WHITE),
            CosmicBackgroundColor(FIELD_BG),
            Transform::from_translation(pos),
            MaxLines(1),
            CosmicTextAlign::TopLeft { padding: 4 },
        ))
        .id()
}

/// 左寄せの説明ラベルを content の子として spawn する。
fn spawn_label(commands: &mut Commands, parent: Entity, text: &str, pos: Vec3) {
    let label = commands
        .spawn((
            Text2d::new(text),
            TextFont {
                font_size: 13.0,
                ..default()
            },
            TextColor(Color::srgb(0.8, 0.8, 0.85)),
            bevy::sprite::Anchor::CenterLeft,
            Transform::from_translation(pos),
        ))
        .id();
    commands.entity(parent).add_child(label);
}

/// ボタン (Sprite + ラベル + Pointer<Click> observer) を content の子として spawn する。
fn spawn_button(
    commands: &mut Commands,
    parent: Entity,
    kind: FindButtonKind,
    label: &str,
    pos: Vec3,
    size: Vec2,
) {
    let btn = commands
        .spawn((
            Sprite {
                custom_size: Some(size),
                color: Color::srgba(0.2, 0.2, 0.32, 1.0),
                ..default()
            },
            Transform::from_translation(pos),
            kind,
        ))
        .observe(
            |trigger: Trigger<Pointer<Click>>,
             kind_q: Query<&FindButtonKind>,
             mut w: EventWriter<FindActionRequested>| {
                if let Ok(k) = kind_q.get(trigger.entity()) {
                    w.send(FindActionRequested(*k));
                }
            },
        )
        .id();

    let txt = commands
        .spawn((
            Text2d::new(label),
            TextFont {
                font_size: 12.0,
                ..default()
            },
            TextColor(Color::WHITE),
            Transform::from_xyz(0.0, 0.0, 0.1),
        ))
        .id();
    commands.entity(btn).add_child(txt);
    commands.entity(parent).add_child(btn);
}

// ─────────────────────────────────────────────────────────────────
// tests
// ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── find_matches ───────────────────────────────────────────

    #[test]
    fn find_matches_empty_query_returns_none() {
        assert!(find_matches("def foo()", "", false).is_empty());
    }

    #[test]
    fn find_matches_single_match() {
        let m = find_matches("def foo():", "foo", true);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].line, 0);
        assert_eq!(m[0].byte_range, 4..7);
    }

    #[test]
    fn find_matches_multiple_per_line_non_overlapping() {
        // "aaaa" with query "aa" → matches at 0..2 and 2..4 (non-overlapping).
        let m = find_matches("aaaa", "aa", true);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].byte_range, 0..2);
        assert_eq!(m[1].byte_range, 2..4);
    }

    #[test]
    fn find_matches_overlap_is_skipped() {
        // "aaa" with query "aa" → only 0..2 (next start 2 leaves single 'a', no match).
        let m = find_matches("aaa", "aa", true);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].byte_range, 0..2);
    }

    #[test]
    fn find_matches_case_insensitive_default() {
        let m = find_matches("DEF Foo", "def", false);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].byte_range, 0..3);
    }

    #[test]
    fn find_matches_case_sensitive_excludes_other_case() {
        let m = find_matches("DEF def", "def", true);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].byte_range, 4..7);
    }

    #[test]
    fn find_matches_spans_multiple_lines() {
        let src = "x = 1\ny = x\nz = x";
        let m = find_matches(src, "x", true);
        // line0: "x = 1" → x at 0; line1: "y = x" → x at 4; line2: "z = x" → x at 4.
        assert_eq!(m.len(), 3);
        assert_eq!((m[0].line, m[0].byte_range.clone()), (0, 0..1));
        assert_eq!((m[1].line, m[1].byte_range.clone()), (1, 4..5));
        assert_eq!((m[2].line, m[2].byte_range.clone()), (2, 4..5));
    }

    // ── apply_replacement ──────────────────────────────────────

    fn ms(line: usize, range: std::ops::Range<usize>) -> MatchSpan {
        MatchSpan {
            line,
            byte_range: range,
        }
    }

    #[test]
    fn apply_replacement_empty_matches_unchanged() {
        let out = apply_replacement("abc", &[], 0, "X", ReplaceScope::All);
        assert_eq!(out, "abc");
    }

    #[test]
    fn apply_replacement_current_only() {
        let src = "foo foo foo";
        let matches = find_matches(src, "foo", true);
        let out = apply_replacement(src, &matches, 1, "BAR", ReplaceScope::Current);
        assert_eq!(out, "foo BAR foo");
    }

    #[test]
    fn apply_replacement_all() {
        let src = "foo foo foo";
        let matches = find_matches(src, "foo", true);
        let out = apply_replacement(src, &matches, 0, "BAR", ReplaceScope::All);
        assert_eq!(out, "BAR BAR BAR");
    }

    #[test]
    fn apply_replacement_right_to_left_keeps_offsets_valid() {
        // replacement longer than match — left-to-right would corrupt later offsets.
        let src = "a b a b";
        let matches = find_matches(src, "b", true);
        let out = apply_replacement(src, &matches, 0, "LONG", ReplaceScope::All);
        assert_eq!(out, "a LONG a LONG");
    }

    #[test]
    fn apply_replacement_across_lines() {
        let src = "x = 1\ny = 2\nx = 3";
        let matches = find_matches(src, "x", true);
        let out = apply_replacement(src, &matches, 0, "Q", ReplaceScope::All);
        assert_eq!(out, "Q = 1\ny = 2\nQ = 3");
    }

    #[test]
    fn apply_replacement_to_empty_string_deletes_match() {
        let src = "foo bar foo";
        let matches = find_matches(src, "foo", true);
        let out = apply_replacement(src, &matches, 0, "", ReplaceScope::All);
        assert_eq!(out, " bar ");
    }

    // ── systems (Bevy App) ─────────────────────────────────────

    use crate::ui::strategy_editor_highlight::FindMatchSpans;

    #[test]
    fn sync_find_editors_writes_query_not_replacement() {
        let mut app = App::new();
        app.init_resource::<FindReplaceState>();
        app.add_event::<CosmicTextChanged>();
        app.add_systems(Update, sync_find_editors_to_state_system);

        let query_e = app.world_mut().spawn(FindQueryEditor).id();
        let repl_e = app.world_mut().spawn(FindReplacementEditor).id();

        app.world_mut()
            .send_event(CosmicTextChanged((query_e, "needle".to_string())));
        app.world_mut()
            .send_event(CosmicTextChanged((repl_e, "rep".to_string())));
        app.update();

        let state = app.world().resource::<FindReplaceState>();
        assert_eq!(state.query, "needle");
        assert_eq!(state.replacement, "rep");
    }

    #[test]
    fn compute_find_match_spans_writes_matches_to_target_editor() {
        let mut app = App::new();
        app.init_resource::<FindReplaceState>();
        app.add_systems(Update, compute_find_match_spans_system);

        let region = "region_001".to_string();
        app.world_mut().spawn((
            WindowRoot,
            StrategyEditorId {
                region_key: region.clone(),
            },
            StrategyFragment {
                source: "def foo():\n    def bar(): pass".to_string(),
                dirty: false,
            },
        ));
        let editor = app
            .world_mut()
            .spawn((
                StrategyEditorContent,
                StrategyEditorId {
                    region_key: region.clone(),
                },
                FindMatchSpans::default(),
            ))
            .id();

        {
            let mut state = app.world_mut().resource_mut::<FindReplaceState>();
            state.is_open = true;
            state.query = "def".to_string();
            state.target_editor = Some(editor);
        }
        app.update();

        let state = app.world().resource::<FindReplaceState>();
        assert_eq!(state.matches.len(), 2, "two `def` occurrences");

        let spans = app.world().get::<FindMatchSpans>(editor).unwrap();
        assert_eq!(spans.matches.len(), 2);
        assert_eq!(spans.current_idx, Some(0));
    }

    #[test]
    fn compute_find_match_spans_clears_on_empty_query() {
        let mut app = App::new();
        app.init_resource::<FindReplaceState>();
        app.add_systems(Update, compute_find_match_spans_system);

        let region = "region_001".to_string();
        app.world_mut().spawn((
            WindowRoot,
            StrategyEditorId {
                region_key: region.clone(),
            },
            StrategyFragment {
                source: "def foo()".to_string(),
                dirty: false,
            },
        ));
        let editor = app
            .world_mut()
            .spawn((
                StrategyEditorContent,
                StrategyEditorId {
                    region_key: region.clone(),
                },
                FindMatchSpans {
                    matches: vec![ms(0, 0..3)],
                    current_idx: Some(0),
                    prev_match_lines: vec![],
                },
            ))
            .id();

        {
            let mut state = app.world_mut().resource_mut::<FindReplaceState>();
            state.is_open = true;
            state.query = String::new(); // empty → clear
            state.target_editor = Some(editor);
        }
        app.update();

        let spans = app.world().get::<FindMatchSpans>(editor).unwrap();
        assert!(spans.matches.is_empty());
        assert_eq!(spans.current_idx, None);
        // 旧マッチ行が prev_match_lines に退避され、composer が base 色へ戻せる。
        assert_eq!(spans.prev_match_lines, vec![0]);
    }

    #[test]
    fn navigation_is_not_reset_by_next_frame_recompute() {
        // Regression: find_navigate mutates state.current (→ FindReplaceState changed),
        // and compute must NOT treat that as a search-input change and snap current back to 0.
        let mut app = App::new();
        app.init_resource::<FindReplaceState>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.insert_resource(FocusedWidget(None));
        app.add_event::<FindActionRequested>();
        app.add_systems(
            Update,
            (compute_find_match_spans_system, find_navigate_system).chain(),
        );

        let region = "region_001".to_string();
        app.world_mut().spawn((
            WindowRoot,
            StrategyEditorId {
                region_key: region.clone(),
            },
            StrategyFragment {
                source: "x x x".to_string(), // 3 single-char matches at bytes 0,2,4
                dirty: false,
            },
        ));
        let editor = app
            .world_mut()
            .spawn((
                StrategyEditorContent,
                StrategyEditorId {
                    region_key: region.clone(),
                },
                FindMatchSpans::default(),
            ))
            .id();

        {
            let mut state = app.world_mut().resource_mut::<FindReplaceState>();
            state.is_open = true;
            state.query = "x".to_string();
            state.target_editor = Some(editor);
        }

        // Frame 1: compute finds 3 matches (current=0), then Next advances current → 1.
        app.world_mut()
            .send_event(FindActionRequested(FindButtonKind::Next));
        app.update();
        assert_eq!(app.world().resource::<FindReplaceState>().matches.len(), 3);
        assert_eq!(
            app.world().resource::<FindReplaceState>().current,
            1,
            "Next should advance to match index 1"
        );

        // Frame 2: no new input, no source change → compute must leave current at 1.
        app.update();
        assert_eq!(
            app.world().resource::<FindReplaceState>().current,
            1,
            "navigation must survive the next-frame recompute"
        );
    }

    #[test]
    fn orphaned_target_keeps_panel_handle_for_despawn() {
        // Regression: closing the target Strategy Editor while Find is open must NOT
        // drop panel_root (otherwise the Find panel entity leaks — never despawned and
        // unclosable). State resets to default but panel handles survive so the lifecycle
        // system's close branch (!is_open && panel_root.is_some()) despawns the panel.
        let mut app = App::new();
        app.init_resource::<FindReplaceState>();
        app.add_systems(Update, compute_find_match_spans_system);

        let region = "region_001".to_string();
        app.world_mut().spawn((
            WindowRoot,
            StrategyEditorId {
                region_key: region.clone(),
            },
            StrategyFragment {
                source: "x x".to_string(),
                dirty: false,
            },
        ));
        let editor = app
            .world_mut()
            .spawn((
                StrategyEditorContent,
                StrategyEditorId {
                    region_key: region.clone(),
                },
                FindMatchSpans::default(),
            ))
            .id();
        // Stand-in panel entities (their identity is what must survive the reset).
        let panel = app.world_mut().spawn_empty().id();
        let query_e = app.world_mut().spawn_empty().id();
        let repl_e = app.world_mut().spawn_empty().id();

        {
            let mut state = app.world_mut().resource_mut::<FindReplaceState>();
            state.is_open = true;
            state.query = "x".to_string();
            state.target_editor = Some(editor);
            state.panel_root = Some(panel);
            state.query_editor = Some(query_e);
            state.replacement_editor = Some(repl_e);
        }
        app.update(); // populates matches for the live target

        // Target editor is closed (×) → despawn it, then recompute.
        app.world_mut().entity_mut(editor).despawn();
        app.update();

        let state = app.world().resource::<FindReplaceState>();
        assert!(!state.is_open, "closing target requests Find close");
        assert_eq!(state.target_editor, None);
        // Panel handles preserved so manage_find_panel_lifecycle_system can despawn the panel.
        assert_eq!(state.panel_root, Some(panel), "panel_root must not leak");
        assert_eq!(state.query_editor, Some(query_e));
        assert_eq!(state.replacement_editor, Some(repl_e));
    }

    #[test]
    fn retarget_clears_old_editor_spans() {
        // Switching target_editor must clear the previous target's highlight (multi-spawn).
        let mut app = App::new();
        app.init_resource::<FindReplaceState>();
        app.add_systems(Update, compute_find_match_spans_system);

        let spawn_pair = |app: &mut App, region: &str| -> Entity {
            app.world_mut().spawn((
                WindowRoot,
                StrategyEditorId {
                    region_key: region.to_string(),
                },
                StrategyFragment {
                    source: "x x".to_string(),
                    dirty: false,
                },
            ));
            app.world_mut()
                .spawn((
                    StrategyEditorContent,
                    StrategyEditorId {
                        region_key: region.to_string(),
                    },
                    FindMatchSpans::default(),
                ))
                .id()
        };
        let editor_a = spawn_pair(&mut app, "region_001");
        let editor_b = spawn_pair(&mut app, "region_002");

        // Target A, find matches.
        {
            let mut state = app.world_mut().resource_mut::<FindReplaceState>();
            state.is_open = true;
            state.query = "x".to_string();
            state.target_editor = Some(editor_a);
        }
        app.update();
        assert_eq!(
            app.world()
                .get::<FindMatchSpans>(editor_a)
                .unwrap()
                .matches
                .len(),
            2
        );

        // Retarget to B → A's spans must be cleared, B's populated.
        app.world_mut()
            .resource_mut::<FindReplaceState>()
            .target_editor = Some(editor_b);
        app.update();
        assert!(
            app.world()
                .get::<FindMatchSpans>(editor_a)
                .unwrap()
                .matches
                .is_empty(),
            "old target spans cleared on retarget"
        );
        assert_eq!(
            app.world()
                .get::<FindMatchSpans>(editor_b)
                .unwrap()
                .matches
                .len(),
            2
        );
    }
}
