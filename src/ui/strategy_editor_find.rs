//! Find / Replace パネル (Slice 5 #50: cosmic 撤去・Bevy UI Node 化済み)。
//!
//! 設計原則:
//! - Find マッチの **計算と描画は分離**: 計算は `compute_find_match_spans_system` が
//!   `FindReplaceState.matches` (ナビ用) と editor entity の `FindMatchSpans` (描画用) を
//!   同時に更新する。描画 overlay は Phase B+1 で `bevy_instanced_text::Overlays` に乗せる予定。
//! - パネル本体は **Bevy UI Node** (`FindPanelRoot`)。入力欄は自前の Text + Events<KeyboardInput>
//!   drain (`find_field_input_system`)、ボタンは `Button + Interaction` から
//!   `FindActionRequested` を emit (`find_button_interaction_system`)。
//! - Replace は純粋関数 `apply_replacement` で新ソースを計算し、bevscode editor entity へ
//!   `SetTextRequested { entity, text }` を 1 件 flush する。fragment 更新・undo 記録・autosave・
//!   再ハイライトは bevscode の `TextBuffer<RopeBuffer>` 経由
//!   (`sync_bevscode_to_strategy_fragment_system`) に委譲する。

use crate::ui::components::{StrategyEditorId, StrategyFragment, WindowRoot};
use crate::ui::strategy_editor::StrategyEditorNode;
use crate::ui::theme::Theme;
use bevscode::prelude::SetTextRequested;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;

// ─────────────────────────────────────────────────────────────────
// Match types (Slice 5 (#50): _highlight.rs から move。_highlight.rs 側は shim re-export のみ)
// ─────────────────────────────────────────────────────────────────

/// Find マッチ 1 件 (行 + 行内 byte range)。
/// Clone は `FindReplaceState.matches` (ナビ用) と `FindMatchSpans.matches` (描画用) の
/// 両方へ同じマッチ列を書き込むために必要 (compute_find_match_spans_system)。
#[derive(Clone, Debug, PartialEq)]
pub struct MatchSpan {
    pub line: usize,
    pub byte_range: std::ops::Range<usize>,
}

/// Find マッチ結果。find/replace の検索 system が書き込み、composer がレンダリングに使う。
#[derive(Component, Default)]
pub struct FindMatchSpans {
    pub matches: Vec<MatchSpan>,
    /// 現在マッチの index。`FindReplaceState.current` の **描画側ミラー** で、
    /// composer はこの entity だけ見れば現在マッチを別色にできる (Resource を読まずに済む)。
    /// nav 側 (`FindReplaceState.current`) と更新は常に lockstep。
    pub current_idx: Option<usize>,
    pub prev_match_lines: Vec<usize>,
}

// ─────────────────────────────────────────────────────────────────
// 状態 / マーカー / イベント
// ─────────────────────────────────────────────────────────────────

/// Slice 5 (#50): Find パネルの入力欄 focus 状態。
/// Bevy UI Node 化に伴い cosmic `FocusedWidget` を使わず内部 enum で focus を管理する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FindFocusedField {
    #[default]
    None,
    Query,
    Replacement,
}

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
    /// Slice 5 (#50): 現在 focus 中の入力欄。`find_field_input_system` が drain した
    /// KeyboardInput をこのフィールドに従って query / replacement に振り分ける。
    pub focused_field: FindFocusedField,
}

/// Slice 5 (#50): Bevy UI Node 化した Find パネルの root marker。
/// `manage_find_panel_lifecycle_system` が `Display::Flex / None` を toggle するときの query 対象。
#[derive(Component)]
pub struct FindPanelRoot;

/// Slice 5 (#50): Find パネル内の query 入力 Text node マーカー (Bevy UI Node 版)。
#[derive(Component)]
pub struct FindQueryFieldUi;

/// Slice 5 (#50): Find パネル内の replacement 入力 Text node マーカー (Bevy UI Node 版)。
#[derive(Component)]
pub struct FindReplacementFieldUi;

/// Slice 5 (#50): match 件数表示の Text node マーカー (Bevy UI Node 版、旧 Text2d 版は world-space)。
#[derive(Component)]
pub struct FindMatchCountUi;

/// Find パネルのボタン種別 (entity に貼って observer から引く)。
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum FindButtonKind {
    Prev,
    Next,
    Replace,
    ReplaceAll,
}

/// ボタン click / キーボードから発行されるアクション要求。
#[derive(Message, Clone, Copy)]
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

/// Ctrl+F で開く / Esc で閉じる。`target_editor` には押下時に focus 中の bevscode Strategy editor
/// (StrategyEditorNode) を保存する。Slice 5 (#50): cosmic `FocusedWidget` から Bevy native
/// `InputFocus` + `StrategyEditorNode` 経路に切替。
pub fn find_keyboard_system(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut cooldown: Local<f32>,
    mut state: ResMut<FindReplaceState>,
    mut input_focus: ResMut<bevy::input_focus::InputFocus>,
    editor_q: Query<(), With<StrategyEditorNode>>,
) {
    *cooldown = (*cooldown - time.delta_secs()).max(0.0);

    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if ctrl && keys.just_pressed(KeyCode::KeyF) && *cooldown <= 0.0 {
        if !state.is_open {
            state.is_open = true;
            // focus 中が bevscode Strategy editor ならそれを対象に。そうでなければ None。
            state.target_editor = input_focus.0.filter(|e| editor_q.contains(*e));
            state.focused_field = FindFocusedField::Query;
            // N1 (#50 followup): bevscode は `On<FocusedInput<KeyboardInput>>` 経由で
            // InputFocus の entity にキーを配送する。Find 入力欄に入れた文字が同フレームに
            // strategy ソースへも挿入されるのを防ぐため、ここで InputFocus をクリアして
            // bevscode への配送を止める (Esc / 復元は target_editor から)。
            input_focus.0 = None;
        } else {
            // 既に開いている: 別の Strategy editor に focus 中ならそれへ retarget。
            // query 欄へ focus を戻す (panel 内 enum focus)。
            if let Some(new_target) = input_focus.0.filter(|e| editor_q.contains(*e)) {
                state.target_editor = Some(new_target);
            }
            state.focused_field = FindFocusedField::Query;
            // N1 (#50 followup): retarget 時も InputFocus を外す。
            input_focus.0 = None;
        }
        *cooldown = 0.5;
    }

    if keys.just_pressed(KeyCode::Escape) && state.is_open {
        state.is_open = false;
        state.focused_field = FindFocusedField::None;
        if let Some(target) = state.target_editor {
            input_focus.0 = Some(target);
        }
    }
}

/// Slice 5 (#50): Bevy UI Node 版 Find パネルのライフサイクル。
/// 初回 open で 1 度だけ spawn → 以降は `Display::Flex / None` で開閉する (despawn しない)。
/// 旧 cosmic + spawn_floating_window 経路 (world-space sprite) を撤去。
///
/// 孤児チェック: panel root が外部から despawn されたら panel_root を None に戻し、次の open で再 spawn する。
pub fn manage_find_panel_lifecycle_system(
    mut commands: Commands,
    mut state: ResMut<FindReplaceState>,
    theme: Res<Theme>,
    existence_q: Query<(), With<FindPanelRoot>>,
    mut node_q: Query<&mut Node, With<FindPanelRoot>>,
) {
    // 孤児チェック: panel_root の Entity が消えていたら、panel handles だけ None に戻す。
    if let Some(root) = state.panel_root
        && existence_q.get(root).is_err()
    {
        state.panel_root = None;
        state.query_editor = None;
        state.replacement_editor = None;
    }

    // 初回 open: Bevy UI Node panel を spawn。
    if state.is_open && state.panel_root.is_none() {
        let (root, query_field, replacement_field) = spawn_find_panel(&mut commands, &theme);
        state.panel_root = Some(root);
        state.query_editor = Some(query_field);
        state.replacement_editor = Some(replacement_field);
        state.focused_field = FindFocusedField::Query;
    }

    // Display::Flex / None の toggle (差分書き込みで spurious Change を立てない)。
    if let Some(root) = state.panel_root
        && let Ok(mut node) = node_q.get_mut(root)
    {
        let target = if state.is_open {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != target {
            node.display = target;
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
    // Slice 5 (#50): cosmic 側 (StrategyEditorContent) ではなく bevscode peer 側
    // (StrategyEditorNode) の FindMatchSpans を更新する。region_key は StrategyEditorNode が直接持つ。
    mut editor_q: Query<(&StrategyEditorNode, &mut FindMatchSpans)>,
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
    let Ok((target_node, mut spans)) = editor_q.get_mut(target) else {
        return;
    };
    let region_key = target_node.region_key.clone();

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
    mut actions: MessageReader<FindActionRequested>,
    input_focus: Res<bevy::input_focus::InputFocus>,
    mut state: ResMut<FindReplaceState>,
    mut editor_q: Query<&mut FindMatchSpans, With<StrategyEditorNode>>,
) {
    if !state.is_open || state.matches.is_empty() {
        return;
    }

    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let f3 = keys.just_pressed(KeyCode::F3);
    let enter = keys.just_pressed(KeyCode::Enter);
    // Slice 5 (#50): query 欄の focus は panel 内 enum 経路で判定 (cosmic FocusedWidget なし)。
    let query_focused = state.focused_field == FindFocusedField::Query;
    // ⚠️ 対象 bevscode editor に focus 中の Enter は bevscode 内蔵の改行処理が走るため、
    // ここで navigation に使うと「改行 + 次マッチ」の二重発火になる。target editor に focus 中は
    // Enter-nav を無効化し、F3 のみで移動する。
    let target_focused = state.target_editor.is_some_and(|t| input_focus.0 == Some(t));
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

/// Replace / Replace All ボタンで `apply_replacement` を実行し、対象 bevscode editor entity へ
/// `SetTextRequested { entity, text }` を 1 件 flush する。fragment 更新・undo 記録・autosave・
/// 再ハイライトは bevscode の `TextBuffer<RopeBuffer>` 経由 (`sync_bevscode_to_strategy_fragment_system`)
/// に委譲する (Slice 5 #50)。
pub fn replace_execute_system(
    mut actions: MessageReader<FindActionRequested>,
    state: Res<FindReplaceState>,
    fragments_q: Query<(&StrategyEditorId, &StrategyFragment), With<WindowRoot>>,
    editor_q: Query<&StrategyEditorNode>,
    mut writer: MessageWriter<SetTextRequested>,
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
    let Ok(node) = editor_q.get(target) else {
        return;
    };
    let region_key = node.region_key.clone();
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

    writer.write(SetTextRequested {
        entity: target,
        text: new_source,
    });
}

/// Slice 5 (#50): Find パネルの入力欄が focus 中のとき `Messages<KeyboardInput>` を drain し、
/// `state.query` または `state.replacement` に typing を書き込む。
/// `instrument_picker::picker_searchbox_input_system` と同じ「drain して後段へ流さない」流派。
/// is_open=false または focused_field=None のときは何もしない (bevscode editor 側の typing と
/// 競合しないため必須)。
/// Slice 5 (#50): Bevy UI Node ボタン (`Button` + `FindButtonKind` marker) の
/// `Interaction::Pressed` エッジで `FindActionRequested(kind)` を発火する。
/// 旧 Sprite + observer 経路の置換 (操作系 UI = Bevy UI Node ルール; `menu_bar::menu_item_system` 流派)。
pub fn find_button_interaction_system(
    q: Query<(&Interaction, &FindButtonKind), Changed<Interaction>>,
    mut writer: MessageWriter<FindActionRequested>,
) {
    for (interaction, kind) in q.iter() {
        if *interaction == Interaction::Pressed {
            writer.write(FindActionRequested(*kind));
        }
    }
}

pub fn find_field_input_system(
    mut kb_events: ResMut<Messages<KeyboardInput>>,
    mut state: ResMut<FindReplaceState>,
    mut input_focus: ResMut<bevy::input_focus::InputFocus>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    if !state.is_open || state.focused_field == FindFocusedField::None {
        return;
    }
    for ev in kb_events.drain() {
        if ev.state != ButtonState::Pressed {
            continue;
        }
        match &ev.logical_key {
            Key::Character(s) => {
                // N4 (#50 followup): Ctrl+F / Cmd+X 等のショートカットを Find 入力欄から
                // 弾く。Alt は除外しない: 欧州配列の AltGr (Windows=Ctrl+Alt / Linux=Alt) で
                // '@' '{' '|' 等の印字文字を入力する経路を潰さないため。Shift は元から許可
                // (大文字)。Ctrl+Alt (AltGr) は Ctrl 一致で skip されるが、これは Windows
                // 環境で AltGr が混入する稀ケースの妥協 (TODO: `ev.text` を見て OS 解決済み
                // テキストを尊重する形に置換)。
                let mod_held = keys.any_pressed([
                    KeyCode::ControlLeft,
                    KeyCode::ControlRight,
                    KeyCode::SuperLeft,
                    KeyCode::SuperRight,
                ]);
                if mod_held {
                    continue;
                }
                let buf = match state.focused_field {
                    FindFocusedField::Query => &mut state.query,
                    FindFocusedField::Replacement => &mut state.replacement,
                    FindFocusedField::None => continue,
                };
                for ch in s.chars() {
                    if !ch.is_control() {
                        buf.push(ch);
                    }
                }
            }
            Key::Backspace => {
                let buf = match state.focused_field {
                    FindFocusedField::Query => &mut state.query,
                    FindFocusedField::Replacement => &mut state.replacement,
                    FindFocusedField::None => continue,
                };
                buf.pop();
            }
            Key::Tab => {
                state.focused_field = match state.focused_field {
                    FindFocusedField::Query => FindFocusedField::Replacement,
                    FindFocusedField::Replacement => FindFocusedField::Query,
                    FindFocusedField::None => FindFocusedField::None,
                };
            }
            Key::Escape => {
                state.is_open = false;
                state.focused_field = FindFocusedField::None;
                // N3 (#50 followup): N1 で InputFocus=None にしたまま panel を閉じると
                // strategy editor に typing が戻らない。target_editor へ focus を復元する
                // (find_keyboard_system の Esc 経路と対称)。
                if let Some(target) = state.target_editor {
                    input_focus.0 = Some(target);
                }
            }
            _ => {}
        }
    }
}

/// 件数表示 ("current/total") を state 変化時に更新する (Slice 5 #50: Bevy UI Node 版 Text に切替)。
pub fn update_find_count_text_system(
    state: Res<FindReplaceState>,
    mut q: Query<&mut Text, With<FindMatchCountUi>>,
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
// spawn ヘルパ (Slice 5 #50: Bevy UI Node ベース)
// ─────────────────────────────────────────────────────────────────

/// Find / Replace パネルの Bevy UI Node ツリーを spawn する。
/// 返り値: (panel_root, query_field_text, replacement_field_text)
///
/// 視覚要素は最小限の skeleton (操作系 UI の機能配線が目的、polish は Phase B+1)。
/// 位置・寸法は screen 固定 (画面右上)、`GlobalZIndex(200)` で menu_bar(100) より前面へ。
fn spawn_find_panel(commands: &mut Commands, theme: &Theme) -> (Entity, Entity, Entity) {
    let panel_bg = theme.colors.panel_background.with_alpha(0.95);
    let field_bg = theme.colors.element_background;
    let btn_bg = theme.colors.element_background;
    let text_fg = theme.colors.text;
    let label_fg = theme.colors.text_muted;
    // Panel root.
    let root = commands
        .spawn((
            FindPanelRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(60.0),
                right: Val::Px(20.0),
                width: Val::Px(380.0),
                padding: UiRect::all(Val::Px(8.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                display: Display::Flex,
                ..default()
            },
            BackgroundColor(panel_bg),
            GlobalZIndex(200),
        ))
        .id();

    // Title.
    let title = commands
        .spawn((
            Text::new("FIND / REPLACE"),
            TextFont {
                font_size: 13.0,
                ..default()
            },
            TextColor(label_fg),
        ))
        .id();
    commands.entity(root).add_child(title);

    // Find row.
    let find_row = spawn_field_row(commands, "Find", FindFieldKind::Query, label_fg, field_bg, text_fg);
    let query_field_text = find_row.text_node;
    commands.entity(root).add_child(find_row.row);

    // Replacement row.
    let repl_row = spawn_field_row(commands, "Repl", FindFieldKind::Replacement, label_fg, field_bg, text_fg);
    let replacement_field_text = repl_row.text_node;
    commands.entity(root).add_child(repl_row.row);

    // Match count + buttons row.
    let bottom_row = commands
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(4.0),
            margin: UiRect::top(Val::Px(4.0)),
            ..default()
        })
        .id();
    let count = commands
        .spawn((
            Text::new("0/0"),
            TextFont {
                font_size: 12.0,
                ..default()
            },
            TextColor(label_fg),
            Node {
                width: Val::Px(50.0),
                ..default()
            },
            FindMatchCountUi,
        ))
        .id();
    commands.entity(bottom_row).add_child(count);
    for (kind, label) in [
        (FindButtonKind::Prev, "<"),
        (FindButtonKind::Next, ">"),
        (FindButtonKind::Replace, "Repl"),
        (FindButtonKind::ReplaceAll, "Repl All"),
    ] {
        let btn = spawn_find_button(commands, kind, label, btn_bg, text_fg);
        commands.entity(bottom_row).add_child(btn);
    }
    commands.entity(root).add_child(bottom_row);

    (root, query_field_text, replacement_field_text)
}

enum FindFieldKind {
    Query,
    Replacement,
}

struct FieldRow {
    row: Entity,
    text_node: Entity,
}

fn spawn_field_row(commands: &mut Commands, label: &str, kind: FindFieldKind, label_color: Color, field_bg: Color, text_color: Color) -> FieldRow {
    let row = commands
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.0),
            ..default()
        })
        .id();
    let label_e = commands
        .spawn((
            Text::new(label),
            TextFont {
                font_size: 12.0,
                ..default()
            },
            TextColor(label_color),
            Node {
                width: Val::Px(40.0),
                ..default()
            },
        ))
        .id();
    let field_container = commands
        .spawn((
            Node {
                width: Val::Px(300.0),
                height: Val::Px(22.0),
                padding: UiRect::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(field_bg),
        ))
        .id();
    let text_node = commands
        .spawn((
            Text::new(""),
            TextFont {
                font_size: 14.0,
                ..default()
            },
            TextColor(text_color),
        ))
        .id();
    match kind {
        FindFieldKind::Query => {
            commands.entity(text_node).insert(FindQueryFieldUi);
        }
        FindFieldKind::Replacement => {
            commands.entity(text_node).insert(FindReplacementFieldUi);
        }
    }
    commands.entity(field_container).add_child(text_node);
    commands.entity(row).add_child(label_e);
    commands.entity(row).add_child(field_container);
    FieldRow { row, text_node }
}

fn spawn_find_button(commands: &mut Commands, kind: FindButtonKind, label: &str, btn_color: Color, text_color: Color) -> Entity {
    let btn = commands
        .spawn((
            Button,
            Node {
                width: Val::Auto,
                height: Val::Px(22.0),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(2.0)),
                margin: UiRect::right(Val::Px(4.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(btn_color),
            kind,
        ))
        .id();
    let txt = commands
        .spawn((
            Text::new(label),
            TextFont {
                font_size: 12.0,
                ..default()
            },
            TextColor(text_color),
        ))
        .id();
    commands.entity(btn).add_child(txt);
    btn
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

    // FindMatchSpans / MatchSpan は同モジュール定義 (super 経由)。

    #[test]
    fn compute_find_match_spans_writes_matches_to_target_editor() {
        let mut app = App::new();
        app.init_resource::<crate::ui::theme::Theme>();
        app.init_resource::<FindReplaceState>();
        app.add_systems(Update, compute_find_match_spans_system);

        let region = "region_001".to_string();
        let root = app
            .world_mut()
            .spawn((
                WindowRoot,
                StrategyEditorId {
                    region_key: region.clone(),
                },
                StrategyFragment {
                    source: "def foo():\n    def bar(): pass".to_string(),
                    dirty: false,
                },
            ))
            .id();
        // Slice 5 (#50): target は bevscode peer (StrategyEditorNode 持ち) entity に切替。
        // cosmic 側 (StrategyEditorContent) は Slice 6 で消えるので新コードからは触らない。
        let editor = app
            .world_mut()
            .spawn((
                StrategyEditorNode {
                    root,
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
        app.init_resource::<crate::ui::theme::Theme>();
        app.init_resource::<FindReplaceState>();
        app.add_systems(Update, compute_find_match_spans_system);

        let region = "region_001".to_string();
        let root = app
            .world_mut()
            .spawn((
                WindowRoot,
                StrategyEditorId {
                    region_key: region.clone(),
                },
                StrategyFragment {
                    source: "def foo()".to_string(),
                    dirty: false,
                },
            ))
            .id();
        let editor = app
            .world_mut()
            .spawn((
                StrategyEditorNode {
                    root,
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
        app.init_resource::<crate::ui::theme::Theme>();
        app.init_resource::<FindReplaceState>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<bevy::input_focus::InputFocus>();
        app.add_message::<FindActionRequested>();
        app.add_systems(
            Update,
            (compute_find_match_spans_system, find_navigate_system).chain(),
        );

        let region = "region_001".to_string();
        let root = app
            .world_mut()
            .spawn((
                WindowRoot,
                StrategyEditorId {
                    region_key: region.clone(),
                },
                StrategyFragment {
                    source: "x x x".to_string(), // 3 single-char matches at bytes 0,2,4
                    dirty: false,
                },
            ))
            .id();
        let editor = app
            .world_mut()
            .spawn((
                StrategyEditorNode {
                    root,
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
            .write_message(FindActionRequested(FindButtonKind::Next));
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
        app.init_resource::<crate::ui::theme::Theme>();
        app.init_resource::<FindReplaceState>();
        app.add_systems(Update, compute_find_match_spans_system);

        let region = "region_001".to_string();
        let root = app
            .world_mut()
            .spawn((
                WindowRoot,
                StrategyEditorId {
                    region_key: region.clone(),
                },
                StrategyFragment {
                    source: "x x".to_string(),
                    dirty: false,
                },
            ))
            .id();
        let editor = app
            .world_mut()
            .spawn((
                StrategyEditorNode {
                    root,
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
        app.init_resource::<crate::ui::theme::Theme>();
        app.init_resource::<FindReplaceState>();
        app.add_systems(Update, compute_find_match_spans_system);

        let spawn_pair = |app: &mut App, region: &str| -> Entity {
            let root = app
                .world_mut()
                .spawn((
                    WindowRoot,
                    StrategyEditorId {
                        region_key: region.to_string(),
                    },
                    StrategyFragment {
                        source: "x x".to_string(),
                        dirty: false,
                    },
                ))
                .id();
            app.world_mut()
                .spawn((
                    StrategyEditorNode {
                        root,
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

    // ── manage_find_panel_lifecycle_system (Slice 5 #50) ───────
    //
    // open=true で初回に FindPanelRoot Node が spawn され Display::Flex、
    // open=false に切り替えると同じ Node の Display::None に toggle (despawn しない)。

    fn count_find_panel_roots(app: &mut App) -> Vec<Entity> {
        let world = app.world_mut();
        let mut q = world.query_filtered::<Entity, With<FindPanelRoot>>();
        q.iter(world).collect()
    }

    #[test]
    fn lifecycle_spawns_panel_on_open_and_toggles_display() {
        let mut app = App::new();
        app.init_resource::<crate::ui::theme::Theme>();
        app.init_resource::<FindReplaceState>();
        app.add_systems(Update, manage_find_panel_lifecycle_system);

        // 初期状態は閉。Node は存在しない。
        app.update();
        assert_eq!(count_find_panel_roots(&mut app).len(), 0);

        // open → Node spawn + Display::Flex
        app.world_mut().resource_mut::<FindReplaceState>().is_open = true;
        app.update();
        let panel_entities = count_find_panel_roots(&mut app);
        assert_eq!(panel_entities.len(), 1, "open で 1 個 spawn");
        let root = panel_entities[0];
        let node = app.world().get::<Node>(root).expect("FindPanelRoot に Node");
        assert_eq!(node.display, Display::Flex);

        // close → 同じ Node が Display::None に
        app.world_mut().resource_mut::<FindReplaceState>().is_open = false;
        app.update();
        let panel_entities2 = count_find_panel_roots(&mut app);
        assert_eq!(panel_entities2.len(), 1, "close でも entity は残る");
        let node = app.world().get::<Node>(root).unwrap();
        assert_eq!(node.display, Display::None);

        // 再度 open → 同じ Node が Display::Flex に戻る
        app.world_mut().resource_mut::<FindReplaceState>().is_open = true;
        app.update();
        let node = app.world().get::<Node>(root).unwrap();
        assert_eq!(node.display, Display::Flex);
    }

    // ── find_button_interaction_system (Slice 5 #50) ───────────
    //
    // `(Button, Interaction::Pressed, FindButtonKind::Next)` を spawn → app.update() →
    // `FindActionRequested(Next)` が 1 件 flush される。

    #[test]
    fn find_button_interaction_emits_action_on_pressed_edge() {
        let mut app = App::new();
        app.init_resource::<crate::ui::theme::Theme>();
        app.add_message::<FindActionRequested>();
        app.add_systems(Update, find_button_interaction_system);

        app.world_mut()
            .spawn((Button, Interaction::Pressed, FindButtonKind::Next));
        app.update();

        let drained: Vec<FindActionRequested> = app
            .world_mut()
            .resource_mut::<Messages<FindActionRequested>>()
            .drain()
            .collect();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].0, FindButtonKind::Next);
    }

    // ── find_field_input_system (Slice 5 #50) ──────────────────
    //
    // focused_field=Query で `Key::Character("a")` を送ると `state.query` が "a" になる。
    // is_open=false / focused_field=None のときは drain しても state を触らない。

    fn make_key_char(s: &str) -> KeyboardInput {
        KeyboardInput {
            key_code: KeyCode::KeyA,
            logical_key: Key::Character(s.into()),
            state: ButtonState::Pressed,
            text: Some(s.into()),
            repeat: false,
            window: Entity::PLACEHOLDER,
        }
    }

    fn make_key(key_code: KeyCode, logical: Key) -> KeyboardInput {
        KeyboardInput {
            key_code,
            logical_key: logical,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        }
    }

    #[test]
    fn find_field_input_escape_closes_panel() {
        let mut app = App::new();
        app.init_resource::<crate::ui::theme::Theme>();
        app.init_resource::<FindReplaceState>();
        app.init_resource::<bevy::input_focus::InputFocus>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.add_message::<KeyboardInput>();
        app.add_systems(Update, find_field_input_system);

        {
            let mut state = app.world_mut().resource_mut::<FindReplaceState>();
            state.is_open = true;
            state.focused_field = FindFocusedField::Query;
        }

        app.world_mut()
            .write_message(make_key(KeyCode::Escape, Key::Escape));
        app.update();

        let state = app.world().resource::<FindReplaceState>();
        assert!(!state.is_open, "Esc で is_open=false");
        assert_eq!(
            state.focused_field,
            FindFocusedField::None,
            "Esc で focus も外れる"
        );
    }

    #[test]
    fn find_field_input_tab_cycles_focus_between_query_and_replacement() {
        let mut app = App::new();
        app.init_resource::<crate::ui::theme::Theme>();
        app.init_resource::<FindReplaceState>();
        app.init_resource::<bevy::input_focus::InputFocus>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.add_message::<KeyboardInput>();
        app.add_systems(Update, find_field_input_system);

        // Query → Tab → Replacement
        {
            let mut state = app.world_mut().resource_mut::<FindReplaceState>();
            state.is_open = true;
            state.focused_field = FindFocusedField::Query;
        }
        app.world_mut().write_message(make_key(KeyCode::Tab, Key::Tab));
        app.update();
        assert_eq!(
            app.world().resource::<FindReplaceState>().focused_field,
            FindFocusedField::Replacement
        );

        // Replacement → Tab → Query
        app.world_mut().write_message(make_key(KeyCode::Tab, Key::Tab));
        app.update();
        assert_eq!(
            app.world().resource::<FindReplaceState>().focused_field,
            FindFocusedField::Query
        );
    }

    #[test]
    fn find_field_input_backspace_removes_last_char_from_focused_field() {
        let mut app = App::new();
        app.init_resource::<crate::ui::theme::Theme>();
        app.init_resource::<FindReplaceState>();
        app.init_resource::<bevy::input_focus::InputFocus>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.add_message::<KeyboardInput>();
        app.add_systems(Update, find_field_input_system);

        {
            let mut state = app.world_mut().resource_mut::<FindReplaceState>();
            state.is_open = true;
            state.focused_field = FindFocusedField::Query;
            state.query = "ab".to_string();
        }

        app.world_mut()
            .write_message(make_key(KeyCode::Backspace, Key::Backspace));
        app.update();

        let state = app.world().resource::<FindReplaceState>();
        assert_eq!(state.query, "a");
    }

    #[test]
    fn find_field_input_appends_char_to_query_when_query_focused() {
        let mut app = App::new();
        app.init_resource::<crate::ui::theme::Theme>();
        app.init_resource::<FindReplaceState>();
        app.init_resource::<bevy::input_focus::InputFocus>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.add_message::<KeyboardInput>();
        app.add_systems(Update, find_field_input_system);

        {
            let mut state = app.world_mut().resource_mut::<FindReplaceState>();
            state.is_open = true;
            state.focused_field = FindFocusedField::Query;
        }

        app.world_mut().write_message(make_key_char("a"));
        app.update();

        let state = app.world().resource::<FindReplaceState>();
        assert_eq!(state.query, "a");
        assert_eq!(state.replacement, "");
    }

    // ── replace_execute_system (Slice 5 #50) ───────────────────
    //
    // bevscode 切替後の不変条件:
    //   ReplaceAll action を投げると `SetTextRequested { entity: target, text: new_source }`
    //   が **1 件だけ** flush される (bevscode 側 sync system が fragment/autosave/AppHistory を駆動)。

    #[test]
    fn replace_execute_emits_set_text_requested_on_replace_all() {
        let mut app = App::new();
        app.init_resource::<crate::ui::theme::Theme>();
        app.init_resource::<FindReplaceState>();
        app.add_message::<FindActionRequested>();
        app.add_message::<SetTextRequested>();
        app.add_systems(Update, replace_execute_system);

        let region = "region_001".to_string();
        let root = app
            .world_mut()
            .spawn((
                WindowRoot,
                StrategyEditorId {
                    region_key: region.clone(),
                },
                StrategyFragment {
                    source: "foo bar foo".to_string(),
                    dirty: false,
                },
            ))
            .id();
        let editor = app
            .world_mut()
            .spawn(StrategyEditorNode {
                root,
                region_key: region.clone(),
            })
            .id();

        {
            let mut state = app.world_mut().resource_mut::<FindReplaceState>();
            state.is_open = true;
            state.target_editor = Some(editor);
            state.query = "foo".to_string();
            state.replacement = "X".to_string();
            // compute は走らせないので matches は直接注入
            state.matches = vec![
                MatchSpan {
                    line: 0,
                    byte_range: 0..3,
                },
                MatchSpan {
                    line: 0,
                    byte_range: 8..11,
                },
            ];
            state.current = 0;
        }

        app.world_mut()
            .write_message(FindActionRequested(FindButtonKind::ReplaceAll));
        app.update();

        let drained: Vec<SetTextRequested> = app
            .world_mut()
            .resource_mut::<Messages<SetTextRequested>>()
            .drain()
            .collect();
        assert_eq!(
            drained.len(),
            1,
            "ReplaceAll で SetTextRequested が 1 件だけ flush される"
        );
        assert_eq!(drained[0].entity, editor);
        assert_eq!(drained[0].text, "X bar X");
    }
}
