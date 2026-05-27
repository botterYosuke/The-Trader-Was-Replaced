//! Code-folding handlers — ToggleFold, Fold, Unfold, FoldAll, UnfoldAll,
//! plus the change-detection system that fans `is_folded` transitions
//! onto the message bus as `FoldStateChanged`.

use bevy_instanced_text_editor::RopeBuffer;
use std::collections::HashMap;

use crate::input::action_events::*;
use crate::types::events::FoldStateChanged;
use crate::types::*;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;

pub fn handle_toggle_fold(
    mut events: MessageReader<ToggleFoldRequested>,
    input_focus: Res<InputFocus>,
    mut q: Query<
        (
            &CursorState,
            &crate::text_view::TextBuffer<RopeBuffer>,
            &mut FoldState,
        ),
        With<CodeEditor>,
    >,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok((cursor, buffer, mut fold_state)) = q.get_mut(entity) else {
        return;
    };
    let line = buffer.char_to_line(cursor.cursor_pos);
    fold_state.toggle_fold_at_line(line);
}

pub fn handle_fold(
    mut events: MessageReader<FoldRequested>,
    input_focus: Res<InputFocus>,
    mut q: Query<
        (
            &CursorState,
            &crate::text_view::TextBuffer<RopeBuffer>,
            &mut FoldState,
        ),
        With<CodeEditor>,
    >,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok((cursor, buffer, mut fold_state)) = q.get_mut(entity) else {
        return;
    };
    let line = buffer.char_to_line(cursor.cursor_pos);
    fold_state.fold_at_line(line);
}

pub fn handle_unfold(
    mut events: MessageReader<UnfoldRequested>,
    input_focus: Res<InputFocus>,
    mut q: Query<
        (
            &CursorState,
            &crate::text_view::TextBuffer<RopeBuffer>,
            &mut FoldState,
        ),
        With<CodeEditor>,
    >,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok((cursor, buffer, mut fold_state)) = q.get_mut(entity) else {
        return;
    };
    let line = buffer.char_to_line(cursor.cursor_pos);
    fold_state.unfold_at_line(line);
}

pub fn handle_fold_all(
    mut events: MessageReader<FoldAllRequested>,
    input_focus: Res<InputFocus>,
    mut q: Query<&mut FoldState, With<CodeEditor>>,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok(mut fold_state) = q.get_mut(entity) else {
        return;
    };
    fold_state.fold_all();
}

pub fn handle_unfold_all(
    mut events: MessageReader<UnfoldAllRequested>,
    input_focus: Res<InputFocus>,
    mut q: Query<&mut FoldState, With<CodeEditor>>,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok(mut fold_state) = q.get_mut(entity) else {
        return;
    };
    fold_state.unfold_all();
}

/// Watches `Changed<FoldState>` and emits one `FoldStateChanged` per
/// region whose `is_folded` flipped since the last frame. The fold-region
/// detector bumps `content_version` (re-parse) without changing fold flags;
/// without per-region diffing hosts would see a flood of false positives on
/// every keystroke.
///
/// Uses a per-entity fingerprint over the folded-line set as a fast-path:
/// async reparse completions rebuild `regions` but preserve `is_folded`, so
/// the fingerprint stays stable and we skip the per-region diff entirely.
/// Without this fast path, sqlite3.c's thousands of regions cost ~8 ms per
/// reparse just to discover nothing flipped.
type FoldStateChangedQuery<'w, 's> =
    Query<'w, 's, (Entity, &'static FoldState), (With<CodeEditor>, Changed<FoldState>)>;

pub fn emit_fold_state_changed(
    q: FoldStateChangedQuery,
    mut writer: MessageWriter<FoldStateChanged>,
    mut last_known: Local<HashMap<(Entity, usize), bool>>,
    mut last_fingerprint: Local<HashMap<Entity, u64>>,
) {
    for (entity, state) in q.iter() {
        // Cheap fingerprint: XOR-fold of folded regions' start_line. Stable
        // under reordering (XOR is commutative) but flips bit-precisely when
        // any region's is_folded flag toggles.
        let mut fp: u64 = 0;
        let mut folded_count: u64 = 0;
        for r in &state.regions {
            if r.is_folded {
                fp ^= (r.start_line as u64).wrapping_mul(0x9E3779B97F4A7C15);
                folded_count += 1;
            }
        }
        let fp = fp ^ folded_count.wrapping_mul(0xBF58476D1CE4E5B9);
        if last_fingerprint.get(&entity).copied() == Some(fp) {
            continue;
        }
        last_fingerprint.insert(entity, fp);

        // Fingerprint changed — fall back to the per-region diff to find
        // exactly which lines flipped.
        let mut seen: HashMap<(Entity, usize), bool> = HashMap::with_capacity(state.regions.len());
        for region in &state.regions {
            let key = (entity, region.start_line);
            seen.insert(key, region.is_folded);
            let prev = last_known.get(&key).copied();
            if prev != Some(region.is_folded) {
                writer.write(FoldStateChanged {
                    entity,
                    start_line: region.start_line,
                    is_folded: region.is_folded,
                });
            }
        }
        // Drop entries for regions that no longer exist (re-parse merged/split
        // them or the editor was despawned). Re-introducing the same start_line
        // counts as a fresh transition, which is the right behavior.
        last_known.retain(|key, _| key.0 != entity || seen.contains_key(key));
        for (key, val) in seen {
            last_known.insert(key, val);
        }
    }
}
