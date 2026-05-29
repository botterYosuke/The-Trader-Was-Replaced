use crate::ui::components::{
    PanelKind, PanelSpawnRequested, PanelSpawnSource, RedoMenuRequested, RegionKeyAllocator,
    StrategyBuffer, StrategyEditorId, StrategyEditorSpawnSpec, StrategyFileLoadRequested,
    StrategyFragment, UndoMenuRequested, WindowRoot,
};
use crate::ui::editor_history::{
    AppEditAction, AppHistory, PendingStrategySnapshotRestore, UndoRedoApplied,
};
use crate::ui::floating_window::{
    FloatingWindowSpec, TITLE_BAR_HEIGHT, spawn_floating_window,
};
use crate::ui::layout_persistence::{AutoSaveState, PendingLayoutApply};
use crate::ui::strategy_editor_find::FindMatchSpans;
use bevy::prelude::*;

// ── Strategy Editor (bevscode 化済 / Slice 6c で cosmic 撤去) ─────────────

const PANEL_SIZE: Vec2 = Vec2::new(500.0, 400.0);
const PANEL_POSITION: Vec2 = Vec2::new(-300.0, 50.0);
const ACCENT: Color = Color::srgba(0.63, 0.44, 1.0, 0.4); // SVG #a070ff (purple)

/// debounce 自動保存の進行状況を追跡する resource。
/// `mark_strategy_dirty` で `last_change` を記録し、`debounced_strategy_autosave_system`
/// が 1 秒経過後に `flush_strategy_cache` を呼んで cache_path へ書き出す。
#[derive(Resource, Default)]
pub struct StrategyAutoSaveState {
    pub dirty: bool,
    pub last_change: Option<std::time::Instant>,
}

/// `StrategyBuffer.source` を `cache_path` に書き出し、dirty 状態をクリアする。
///
/// 戻り値:
/// - `Ok(true)`: 書き出し成功、`buffer.dirty` と `auto_save` の dirty/last_change をリセット
/// - `Ok(false)`: `cache_path` 未設定でスキップ（state は不変）
/// - `Err(e)`: I/O 失敗、state は不変（呼び出し側で error log / 再試行）
pub fn flush_strategy_cache(
    merged: &str,
    buffer: &mut StrategyBuffer,
    auto_save: &mut StrategyAutoSaveState,
) -> std::io::Result<bool> {
    let Some(path) = buffer.cache_path.as_ref() else {
        return Ok(false);
    };
    std::fs::write(path, merged)?;
    buffer.last_merged_source = Some(merged.to_string());
    auto_save.dirty = false;
    auto_save.last_change = None;
    Ok(true)
}

/// debounce 判定。dirty かつ最後の変更から `debounce` 経過なら true。
///
/// `saturating_duration_since` を使うことで now < last_change（クロック逆転）でも panic しない。
fn should_flush(
    state: &StrategyAutoSaveState,
    now: std::time::Instant,
    debounce: std::time::Duration,
) -> bool {
    if !state.dirty {
        return false;
    }
    let Some(last_change) = state.last_change else {
        return false;
    };
    now.saturating_duration_since(last_change) >= debounce
}

/// エディタ入力 / undo/redo / snapshot restore でテキストが変わった瞬間に呼ぶ。
/// buffer と auto_save の dirty 状態を一括更新して中間状態を作らない。
fn mark_fragment_dirty(
    fragment: &mut StrategyFragment,
    auto_save: &mut StrategyAutoSaveState,
    new_source: String,
) {
    fragment.source = new_source;
    fragment.dirty = true;
    auto_save.dirty = true;
    auto_save.last_change = Some(std::time::Instant::now());
}

/// LiveManual 中に Strategy Editor ウィンドウを隠す際、隠す直前の `Visibility` を退避する marker。
/// Manual を抜けたら保存値へ復元して marker を除去する (issue #31: save/restore 方式)。
/// layout_persistence が `visible:false` で復元したウィンドウは `Hidden` が保存されるため、
/// Manual を抜けても layout の意図どおり隠れたままになる (layout が権威)。
#[derive(Component)]
pub struct StrategyEditorModeHidden(pub Visibility);

/// `ExecutionMode::LiveManual` のときだけ Strategy Editor を隠すシステム。
///
/// - フローティングウィンドウ (`WindowRoot` + `PanelKind::StrategyEditor`): Manual 突入時に
///   現在の `Visibility` を `StrategyEditorModeHidden` に退避して `Hidden` にし、Manual 中は
///   毎フレーム `Hidden` を維持する (他の writer による復活を防ぐ)。Manual を抜けたら退避値へ
///   戻して marker を除去する。
/// - サイドバー "Panels" の Strategy Editor ボタン (`Button` + `PanelKind::StrategyEditor`):
///   Manual のとき `Display::None`、それ以外は `Display::Flex`。
///
/// `is_changed()` ゲートは張らない: Manual 中に新規 spawn されたウィンドウ
/// (file open / cache restore / layout load) も捕捉するため毎フレーム diff-write する。
pub fn apply_strategy_editor_mode_visibility_system(
    exec_mode: Res<crate::trading::ExecutionModeRes>,
    mut commands: Commands,
    mut win_q: Query<
        (
            Entity,
            &PanelKind,
            &mut Visibility,
            Option<&StrategyEditorModeHidden>,
        ),
        With<WindowRoot>,
    >,
    mut btn_q: Query<(&PanelKind, &mut Node), With<Button>>,
) {
    let manual = matches!(exec_mode.mode, crate::trading::ExecutionMode::LiveManual);

    for (entity, kind, mut vis, saved) in &mut win_q {
        if *kind != PanelKind::StrategyEditor {
            continue;
        }
        if manual {
            // 初回だけ現在値を退避。以降は Hidden を維持する。
            if saved.is_none() {
                commands
                    .entity(entity)
                    .insert(StrategyEditorModeHidden(*vis));
            }
            if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
            }
        } else if let Some(saved) = saved {
            // Manual を抜けた: 退避値へ復元して marker 除去。
            if *vis != saved.0 {
                *vis = saved.0;
            }
            commands
                .entity(entity)
                .remove::<StrategyEditorModeHidden>();
        }
    }

    let display = if manual {
        Display::None
    } else {
        Display::Flex
    };
    for (kind, mut node) in &mut btn_q {
        if *kind != PanelKind::StrategyEditor {
            continue;
        }
        if node.display != display {
            node.display = display;
        }
    }
}

/// dispatcher から呼ばれる spawn 関数。
pub fn spawn_strategy_editor_panel(
    commands: &mut Commands,
    allocator: &mut RegionKeyAllocator,
    spec: StrategyEditorSpawnSpec,
) {
    // region_key 決定: 外部指定があれば allocator を追従、なければ払い出す。
    // 追従しないと sidecar / undo redo で復元した region_005 と allocator.next=1 が
    // 衝突し、次の blank spawn が region_001 を払い出して既存と被る。
    let region_key = match spec.region_key {
        Some(k) => {
            if let Some(n) = numeric_suffix_of(&k) {
                allocator.bump_to_at_least(n);
            }
            k
        }
        None => allocator.allocate(),
    };

    // seed テキスト: dispatcher が PendingStrategyFragments の drain を済ませて
    // spec.source を確定する責務。本関数は受け取った spec.source をそのまま採用する。
    let seed = spec.source.unwrap_or_default();

    let (root, _content_area, title_bar) = spawn_floating_window(
        commands,
        FloatingWindowSpec {
            title: "STRATEGY EDITOR".to_string(),
            size: PANEL_SIZE,
            position: PANEL_POSITION,
            accent: ACCENT,
            closeable: true,
            resizable: true,
        },
    );
    commands.entity(root).insert((
        PanelKind::StrategyEditor,
        StrategyEditorId {
            region_key: region_key.clone(),
        },
        StrategyFragment {
            source: seed.clone(),
            dirty: false,
        },
        // Slice 1 (#50): projection system が world rect を screen rect に投影する起点。
        // ADR 0006 の Projected Node 方式。cosmic と並存中だが bevscode 側はこの marker から root を辿る。
        StrategyEditorRoot,
    ));

    // ── Slice 1 (#50): bevscode CodeEditor peer の spawn は `spawn_bevscode_peer_on_strategy_editor_added`
    // システムに委譲する。spawn_strategy_editor_panel は AssetServer を持たないが、bevscode のフォント
    // 読み込みに必要なため、root 生成 → 次フレームで Added<StrategyEditorRoot> を watch する system が
    // 拾って peer を spawn する設計にする。
    // 同フレームに peer を建てたい欲求はあるが、root 生成と peer spawn を分けることで
    // AssetServer 依存を `spawn_bevscode_peer_on_strategy_editor_added` 側に隔離できる。
    // 1 フレーム遅延は視覚的に許容。

    let _ = title_bar;
    let _ = spec.layout_source;
}

/// Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z で Undo/Redo を実行する system。
/// `replaying_depth` を +1 してから record.undo/redo を呼び、
/// `UndoRedoApplied` イベントを送信する。
/// `-1` は `apply_pending_app_edits_system` の drain 完了後に行う。
pub fn undo_redo_system(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut cooldown: Local<f32>,
    mut history: ResMut<AppHistory>,
    mut undo_menu_ev: MessageReader<UndoMenuRequested>,
    mut redo_menu_ev: MessageReader<RedoMenuRequested>,
    // UndoRedoApplied は apply_pending_app_edits_system がテキスト変更時のみ送る。
    // ここで送ると Window spawn/despawn undo でも editor set_text が走りカーソルリセットが起きる。
) {
    let menu_undo = undo_menu_ev.read().next().is_some();
    let menu_redo = redo_menu_ev.read().next().is_some();

    *cooldown = (*cooldown - time.delta_secs()).max(0.0);
    if *cooldown > 0.0 && !menu_undo && !menu_redo {
        return;
    }

    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);

    let do_undo = menu_undo || (ctrl && keys.just_pressed(KeyCode::KeyZ) && !shift);
    let do_redo = menu_redo
        || (ctrl
            && (keys.just_pressed(KeyCode::KeyY) || (keys.just_pressed(KeyCode::KeyZ) && shift)));

    if do_undo {
        history.replaying_depth += 1;
        let changed = {
            let AppHistory {
                record, pending, ..
            } = &mut *history;
            record.undo(pending).is_some()
        };
        if !changed {
            history.replaying_depth -= 1; // 何も起きなかったので即戻す
        }
        *cooldown = 0.05;
    } else if do_redo {
        history.replaying_depth += 1;
        let changed = {
            let AppHistory {
                record, pending, ..
            } = &mut *history;
            record.redo(pending).is_some()
        };
        if !changed {
            history.replaying_depth -= 1;
        }
        *cooldown = 0.05;
    }
}

/// pending キューを drain して ECS に反映する system。
/// `replaying_depth` を drain 完了後に -1 する。
/// テキスト変更があった かつ replaying 中のときのみ `UndoRedoApplied` を送信する。
/// （Window spawn/despawn undo ではエディタ set_text が走らないよう条件を絞る）
#[allow(clippy::too_many_arguments)]
pub fn apply_pending_app_edits_system(
    mut history: ResMut<AppHistory>,
    mut fragments_q: Query<(&StrategyEditorId, &mut StrategyFragment), With<WindowRoot>>,
    mut windows_q: Query<(Entity, &PanelKind, &mut Transform), With<WindowRoot>>,
    editor_id_q: Query<(Entity, &StrategyEditorId), With<WindowRoot>>,
    mut commands: Commands,
    mut spawn_ev: MessageWriter<PanelSpawnRequested>,
    mut pending_layout: ResMut<PendingLayoutApply>,
    mut pending_restore: ResMut<PendingStrategySnapshotRestore>,
    mut undo_applied: MessageWriter<UndoRedoApplied>,
    mut auto_save: ResMut<StrategyAutoSaveState>,
    mut layout_auto_save: ResMut<AutoSaveState>,
) {
    if history.pending.queue.is_empty() {
        return;
    }

    let mut any_text = false;
    let actions: Vec<_> = history.pending.queue.drain(..).collect();
    for action in actions {
        match action {
            AppEditAction::SetStrategySource { region_key, text } => {
                history.suppress_echo(region_key.clone(), text.clone());
                if let Some((_, mut fragment)) = fragments_q
                    .iter_mut()
                    .find(|(id, _)| id.region_key == region_key)
                {
                    mark_fragment_dirty(&mut fragment, &mut auto_save, text);
                    any_text = true;
                } else {
                    warn!(
                        "SetStrategySource for region '{}' but no matching root",
                        region_key
                    );
                }
            }
            AppEditAction::MoveWindow {
                kind,
                region_key,
                position,
            } => {
                let target_entity: Option<Entity> = if let Some(rk) = &region_key {
                    editor_id_q
                        .iter()
                        .find(|(_, id)| id.region_key == *rk)
                        .map(|(e, _)| e)
                } else {
                    windows_q
                        .iter()
                        .find(|(_, k, _)| **k == kind)
                        .map(|(e, _, _)| e)
                };
                if let Some(entity) = target_entity {
                    if let Ok((_, _, mut tf)) = windows_q.get_mut(entity) {
                        tf.translation.x = position.x;
                        tf.translation.y = position.y;
                    }
                }
                layout_auto_save.dirty = true;
            }
            AppEditAction::SpawnWindow {
                layout,
                strategy_snapshot,
            } => {
                let strategy_spec = if layout.kind == PanelKind::StrategyEditor {
                    Some(StrategyEditorSpawnSpec {
                        region_key: layout.region_key.clone(),
                        source: None,
                        layout_source: PanelSpawnSource::UndoRedo,
                    })
                } else {
                    None
                };
                spawn_ev.write(PanelSpawnRequested {
                    kind: layout.kind,
                    source: PanelSpawnSource::UndoRedo,
                    strategy_spec,
                });
                pending_layout.windows.push(layout.clone());
                if layout.kind == PanelKind::StrategyEditor
                    && let Some(snap) = strategy_snapshot
                {
                    pending_restore.snapshot = Some(snap);
                }
                layout_auto_save.dirty = true;
            }
            AppEditAction::DespawnWindow { kind, region_key } => {
                let target_entity: Option<Entity> = if let Some(rk) = &region_key {
                    editor_id_q
                        .iter()
                        .find(|(_, id)| id.region_key == *rk)
                        .map(|(e, _)| e)
                } else {
                    windows_q
                        .iter()
                        .find(|(_, k, _)| **k == kind)
                        .map(|(e, _, _)| e)
                };
                if let Some(entity) = target_entity {
                    commands.entity(entity).despawn();
                    layout_auto_save.dirty = true;
                }
            }
        }
    }

    if any_text && history.is_replaying() {
        undo_applied.write(UndoRedoApplied);
    }

    if history.replaying_depth > 0 {
        history.replaying_depth -= 1;
    }
}

/// `PendingStrategySnapshotRestore` にスナップショットが積まれていたら
/// buffer.source を復元し、エディタに反映するトリガーとして `UndoRedoApplied` を発火する。
/// StrategyEditorRoot entity が生成されるまで待つ（2 段階遅延、bevscode peer が後追いで張られるので
/// root の存在で代用する）。
pub fn apply_strategy_snapshot_restore_system(
    mut pending_restore: ResMut<PendingStrategySnapshotRestore>,
    mut fragments_q: Query<(&StrategyEditorId, &mut StrategyFragment), With<WindowRoot>>,
    mut history: ResMut<AppHistory>,
    editor_q: Query<Entity, With<StrategyEditorRoot>>,
    mut undo_applied: MessageWriter<UndoRedoApplied>,
    mut auto_save: ResMut<StrategyAutoSaveState>,
) {
    if pending_restore.snapshot.is_none() {
        return;
    }
    if editor_q.is_empty() {
        return;
    }
    if let Some((region_key, source)) = pending_restore.snapshot.take() {
        history.suppress_echo(region_key.clone(), source.clone());
        if let Some((_, mut fragment)) = fragments_q
            .iter_mut()
            .find(|(id, _)| id.region_key == region_key)
        {
            mark_fragment_dirty(&mut fragment, &mut auto_save, source);
            undo_applied.write(UndoRedoApplied);
        } else {
            warn!(
                "snapshot restore for region '{}' but no matching root yet",
                region_key
            );
            pending_restore.snapshot = Some((region_key, source));
        }
    }
}

/// 1 秒 debounce で `StrategyBuffer` を `cache_path` に自動保存する system。
///
/// 毎フレーム `should_flush` で経過時間を判定し、満たしたときだけ `flush_strategy_cache` を呼ぶ。
/// `cache_path` 未設定 (`Ok(false)`) のときは debounce タイマーをクリアして無限ループを防ぐ。
/// I/O 失敗時は state を保持し、次の debounce 経過で再試行する。
pub fn debounced_strategy_autosave_system(
    mut fragments_q: Query<(&StrategyEditorId, &mut StrategyFragment), With<WindowRoot>>,
    mut buffer: ResMut<StrategyBuffer>,
    mut auto_save: ResMut<StrategyAutoSaveState>,
) {
    const DEBOUNCE: std::time::Duration = std::time::Duration::from_secs(1);
    if !should_flush(&auto_save, std::time::Instant::now(), DEBOUNCE) {
        return;
    }
    let mut items: Vec<(String, String)> = fragments_q
        .iter()
        .map(|(id, f)| (id.region_key.clone(), f.source.clone()))
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    items.retain(|(_, src)| !src.trim().is_empty());
    let merged = merge_fragments(&items);

    match flush_strategy_cache(&merged, &mut buffer, &mut auto_save) {
        Ok(true) => {
            for (_, mut fragment) in fragments_q.iter_mut() {
                fragment.dirty = false;
            }
            info!("strategy cache autosaved: {:?}", buffer.cache_path);
        }
        Ok(false) => {
            auto_save.dirty = false;
            auto_save.last_change = None;
        }
        Err(e) => error!("strategy cache autosave failed: {}", e),
    }
}

// ─── Phase B: merge/split 純粋関数 ──────────────────────────────────────────

/// `split_py_into_fragments` の出力。
pub struct SplitOutcome {
    /// (region_key, source_body) の順序付きリスト。
    /// body は末尾 `\n` を strip 済み・マーカー行を除く。
    pub fragments: Vec<(String, String)>,
    /// `region_NNN` 形式の key から取り出した最大の N 値。
    /// `RegionKeyAllocator::bump_to_at_least` に渡してアロケーターを進める。
    pub max_numeric_suffix: u32,
    /// 警告メッセージ。呼び出し側が `warn!` でログに出す。
    pub warnings: Vec<String>,
}

/// フラグメントリストを Python ソース文字列にマージする。
///
/// 各アイテムを `# region <key>\n<body>\n# endregion <key>\n` に変換して連結する。
/// body が空のときは中間改行なしで `# region <key>\n# endregion <key>\n`。
pub fn merge_fragments(items: &[(String, String)]) -> String {
    let mut out = String::new();
    for (key, body) in items {
        out.push_str("# region ");
        out.push_str(key);
        out.push('\n');
        if !body.is_empty() {
            out.push_str(body);
            out.push('\n');
        }
        out.push_str("# endregion ");
        out.push_str(key);
        out.push('\n');
    }
    out
}

/// `region_NNN` 形式の key から NNN を u32 で返す。マッチしない場合は None。
pub(crate) fn numeric_suffix_of(key: &str) -> Option<u32> {
    key.strip_prefix("region_")
        .and_then(|s| s.parse::<u32>().ok())
}

/// フラグメントをリストに追加する。重複 key は `<key>_dupN` にリネームして追加する。
fn push_fragment_inner(
    fragments: &mut Vec<(String, String)>,
    seen: &mut std::collections::HashMap<String, u32>,
    raw_key: String,
    body_lines: Vec<&str>,
    warnings: &mut Vec<String>,
) {
    let count = seen.entry(raw_key.clone()).or_insert(0);
    let actual_key = if *count == 0 {
        raw_key.clone()
    } else {
        let dup_key = format!("{}_dup{}", raw_key, count);
        warnings.push(format!(
            "duplicate region_key '{}'; renamed to '{}'",
            raw_key, dup_key
        ));
        dup_key
    };
    *count += 1;
    let body = body_lines.join("\n");
    let body = body.trim_end_matches('\n').to_string();
    fragments.push((actual_key, body));
}

/// Python ソース文字列を `# region` / `# endregion` マーカーで断片に分割する。
pub fn split_py_into_fragments(py: &str) -> SplitOutcome {
    fn parse_region(line: &str) -> Option<&str> {
        let key = line.trim_start().strip_prefix("# region ")?.trim();
        if key.is_empty() { None } else { Some(key) }
    }

    fn parse_endregion(line: &str) -> Option<Option<&str>> {
        let rest = line.trim_start().strip_prefix("# endregion")?;
        let key = rest.trim();
        Some(if key.is_empty() { None } else { Some(key) })
    }

    let mut fragments: Vec<(String, String)> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut seen: std::collections::HashMap<String, u32> = std::collections::HashMap::new();

    let mut open_key: Option<String> = None;
    let mut open_body: Vec<&str> = Vec::new();
    let mut preamble: Vec<&str> = Vec::new();
    let mut any_marker = false;

    for line in py.lines() {
        if let Some(region_key) = parse_region(line) {
            if !any_marker && !preamble.is_empty() {
                let preamble_body = preamble.join("\n");
                let preamble_body = preamble_body.trim_end_matches('\n').to_string();
                let preamble_key = "region_001_preamble".to_string();
                let cnt = seen.entry(preamble_key.clone()).or_insert(0);
                *cnt += 1;
                fragments.push((preamble_key.clone(), preamble_body));
                warnings.push(format!(
                    "preamble lines before first # region wrapped into '{}'",
                    preamble_key
                ));
                preamble.clear();
            }
            any_marker = true;

            if let Some(prev_key) = open_key.take() {
                warnings.push(format!(
                    "# region '{}' opened while '{}' was still open; implicitly closing '{}'",
                    region_key, prev_key, prev_key
                ));
                let body = std::mem::take(&mut open_body);
                push_fragment_inner(&mut fragments, &mut seen, prev_key, body, &mut warnings);
            }

            open_key = Some(region_key.to_string());
            open_body = Vec::new();
        } else if let Some(end_key_opt) = parse_endregion(line) {
            any_marker = true;
            match open_key.take() {
                None => {
                    warnings.push(format!(
                        "# endregion '{}' without matching # region; ignored",
                        end_key_opt.unwrap_or("")
                    ));
                }
                Some(cur_key) => {
                    if let Some(ek) = end_key_opt {
                        if ek != cur_key {
                            warnings.push(format!(
                                "# endregion key '{}' does not match open '{}'; closing '{}' anyway",
                                ek, cur_key, cur_key
                            ));
                        }
                    }
                    let body = std::mem::take(&mut open_body);
                    push_fragment_inner(&mut fragments, &mut seen, cur_key, body, &mut warnings);
                }
            }
        } else {
            if open_key.is_some() {
                open_body.push(line);
            } else if !any_marker {
                preamble.push(line);
            }
        }
    }

    if let Some(cur_key) = open_key.take() {
        warnings.push(format!(
            "# region '{}' has no matching # endregion; closed at EOF",
            cur_key
        ));
        let body = std::mem::take(&mut open_body);
        push_fragment_inner(&mut fragments, &mut seen, cur_key, body, &mut warnings);
    }

    if fragments.is_empty() {
        let body = py.trim_end_matches('\n').to_string();
        fragments.push(("region_001".to_string(), body));
    }

    let max_numeric_suffix = fragments
        .iter()
        .filter_map(|(k, _)| numeric_suffix_of(k))
        .max()
        .unwrap_or(0);

    SplitOutcome {
        fragments,
        max_numeric_suffix,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // ── Phase B: merge/split 純粋関数テスト ─────────────────────────────────

    fn ks(s: &str) -> String {
        s.to_string()
    }

    #[test]
    fn merge_fragments_round_trips_through_split() {
        let items = vec![
            (ks("region_001"), ks("x = 1\ny = 2")),
            (ks("region_002"), ks("def foo():\n    pass")),
        ];
        let merged = merge_fragments(&items);
        let outcome = split_py_into_fragments(&merged);
        assert!(
            outcome.warnings.is_empty(),
            "unexpected warnings: {:?}",
            outcome.warnings
        );
        assert_eq!(outcome.fragments, items);
    }

    #[test]
    fn split_py_handles_no_markers_returns_single_region() {
        let py = "x = 1\ny = 2\n";
        let outcome = split_py_into_fragments(py);
        assert!(
            outcome.warnings.is_empty(),
            "expected no warnings: {:?}",
            outcome.warnings
        );
        assert_eq!(outcome.fragments.len(), 1);
        assert_eq!(outcome.fragments[0].0, "region_001");
        assert_eq!(outcome.fragments[0].1, "x = 1\ny = 2");
        assert_eq!(outcome.max_numeric_suffix, 1);
    }

    #[test]
    fn split_py_handles_preamble_warns_and_wraps() {
        let py = "import os\n# region region_002\ncode()\n# endregion region_002\n";
        let outcome = split_py_into_fragments(py);
        assert!(!outcome.warnings.is_empty(), "expected preamble warning");
        assert!(outcome.warnings.iter().any(|w| w.contains("preamble")));
        assert_eq!(outcome.fragments[0].0, "region_001_preamble");
        assert_eq!(outcome.fragments[0].1, "import os");
        assert_eq!(outcome.fragments[1].0, "region_002");
        assert_eq!(outcome.fragments[1].1, "code()");
    }

    #[test]
    fn split_py_handles_duplicate_region_keys() {
        let py = "# region region_001\nalpha\n# endregion region_001\n\
                  # region region_001\nbeta\n# endregion region_001\n";
        let outcome = split_py_into_fragments(py);
        assert!(
            outcome.warnings.iter().any(|w| w.contains("duplicate")),
            "expected dup warning: {:?}",
            outcome.warnings
        );
        assert_eq!(outcome.fragments[0].0, "region_001");
        assert_eq!(outcome.fragments[0].1, "alpha");
        assert_eq!(outcome.fragments[1].0, "region_001_dup1");
        assert_eq!(outcome.fragments[1].1, "beta");
    }

    #[test]
    fn split_py_handles_unmatched_endregion() {
        let py = "# region region_001\ncode\n# endregion region_002\n";
        let outcome = split_py_into_fragments(py);
        assert!(
            outcome
                .warnings
                .iter()
                .any(|w| w.contains("does not match")),
            "warnings: {:?}",
            outcome.warnings
        );
        assert_eq!(outcome.fragments.len(), 1);
        assert_eq!(outcome.fragments[0].0, "region_001");
        assert_eq!(outcome.fragments[0].1, "code");
    }

    #[test]
    fn split_py_handles_orphan_region_at_eof() {
        let py = "# region region_001\norphan line\n";
        let outcome = split_py_into_fragments(py);
        assert!(
            outcome
                .warnings
                .iter()
                .any(|w| w.contains("no matching # endregion")),
            "warnings: {:?}",
            outcome.warnings
        );
        assert_eq!(outcome.fragments.len(), 1);
        assert_eq!(outcome.fragments[0].0, "region_001");
        assert_eq!(outcome.fragments[0].1, "orphan line");
    }

    #[test]
    fn region_key_allocator_bump_to_at_least() {
        let mut alloc = RegionKeyAllocator::default();
        alloc.bump_to_at_least(5);
        assert_eq!(alloc.next, 5);
        alloc.bump_to_at_least(3);
        assert_eq!(alloc.next, 5);
        let k = alloc.allocate();
        assert_eq!(k, "region_006");
        assert_eq!(alloc.next, 6);
    }

    #[test]
    fn merge_fragments_empty_body() {
        let items = vec![(ks("region_001"), ks(""))];
        let merged = merge_fragments(&items);
        assert_eq!(merged, "# region region_001\n# endregion region_001\n");
        let outcome = split_py_into_fragments(&merged);
        assert!(outcome.warnings.is_empty());
        assert_eq!(outcome.fragments, items);
    }

    #[test]
    fn split_py_nested_open_warns_and_implicitly_closes_prev() {
        let py = "# region region_001\nfoo\n\
                  # region region_002\nbar\n# endregion region_002\n";
        let outcome = split_py_into_fragments(py);
        assert!(
            outcome
                .warnings
                .iter()
                .any(|w| w.contains("implicitly closing")),
            "warnings: {:?}",
            outcome.warnings
        );
        assert_eq!(outcome.fragments[0].0, "region_001");
        assert_eq!(outcome.fragments[0].1, "foo");
        assert_eq!(outcome.fragments[1].0, "region_002");
        assert_eq!(outcome.fragments[1].1, "bar");
    }

    use std::fs;
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    #[test]
    fn flush_returns_false_when_cache_path_is_none() {
        let mut buffer = StrategyBuffer {
            original_path: None,
            cache_path: None,
            last_merged_source: None,
        };
        let mut auto_save = StrategyAutoSaveState {
            dirty: true,
            last_change: Some(Instant::now()),
        };

        let result = flush_strategy_cache("fn main() {}", &mut buffer, &mut auto_save);

        assert!(matches!(result, Ok(false)));
        assert!(auto_save.dirty);
        assert!(auto_save.last_change.is_some());
    }

    #[test]
    fn flush_writes_file_and_clears_state() {
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("strategy.rs");
        let content = "fn main() { println!(\"hello\"); }";

        let mut buffer = StrategyBuffer {
            original_path: None,
            cache_path: Some(cache_path.clone()),
            last_merged_source: None,
        };
        let mut auto_save = StrategyAutoSaveState {
            dirty: true,
            last_change: Some(Instant::now()),
        };

        let result = flush_strategy_cache(content, &mut buffer, &mut auto_save);

        assert_eq!(result.unwrap(), true);
        assert!(cache_path.exists());
        let written = fs::read_to_string(&cache_path).unwrap();
        assert_eq!(written, content);
        assert_eq!(buffer.last_merged_source, Some(content.to_string()));
        assert!(!auto_save.dirty);
        assert_eq!(auto_save.last_change, None);
    }

    #[test]
    fn flush_returns_err_and_keeps_state_when_path_unwritable() {
        let temp_dir = TempDir::new().unwrap();
        let unwritable_path = temp_dir.path().join("does_not_exist").join("strategy.rs");

        let mut buffer = StrategyBuffer {
            original_path: None,
            cache_path: Some(unwritable_path),
            last_merged_source: None,
        };
        let now = Instant::now();
        let mut auto_save = StrategyAutoSaveState {
            dirty: true,
            last_change: Some(now),
        };

        let result = flush_strategy_cache("fn main() {}", &mut buffer, &mut auto_save);

        assert!(result.is_err());
        assert!(auto_save.dirty);
        assert_eq!(auto_save.last_change, Some(now));
    }

    #[test]
    fn should_flush_false_when_not_dirty() {
        let state = StrategyAutoSaveState {
            dirty: false,
            last_change: Some(Instant::now()),
        };
        assert!(!should_flush(
            &state,
            Instant::now(),
            Duration::from_secs(1)
        ));
    }

    #[test]
    fn should_flush_false_when_last_change_none() {
        let state = StrategyAutoSaveState {
            dirty: true,
            last_change: None,
        };
        assert!(!should_flush(
            &state,
            Instant::now(),
            Duration::from_secs(1)
        ));
    }

    #[test]
    fn should_flush_false_when_within_debounce() {
        let now = Instant::now();
        let state = StrategyAutoSaveState {
            dirty: true,
            last_change: Some(now),
        };
        assert!(!should_flush(&state, now, Duration::from_millis(500)));
    }

    #[test]
    fn should_flush_true_when_debounce_elapsed() {
        let last_change = Instant::now();
        let now = last_change + Duration::from_secs(2);
        let state = StrategyAutoSaveState {
            dirty: true,
            last_change: Some(last_change),
        };
        assert!(should_flush(&state, now, Duration::from_millis(500)));
    }

    #[test]
    fn mark_fragment_dirty_updates_state() {
        let mut fragment = StrategyFragment {
            source: "old source".to_string(),
            dirty: false,
        };
        let mut auto_save = StrategyAutoSaveState {
            dirty: false,
            last_change: None,
        };

        let new_source = "new source code".to_string();
        mark_fragment_dirty(&mut fragment, &mut auto_save, new_source.clone());

        assert_eq!(fragment.source, new_source);
        assert!(fragment.dirty);
        assert!(auto_save.dirty);
        assert!(auto_save.last_change.is_some());
    }

    #[test]
    fn apply_pending_app_edits_sets_autosave_dirty_on_strategy_source_action() {
        use crate::ui::components::WindowRoot;
        let mut app = App::new();
        app.init_resource::<StrategyBuffer>();
        app.init_resource::<AppHistory>();
        app.init_resource::<StrategyAutoSaveState>();
        app.init_resource::<AutoSaveState>();
        app.init_resource::<PendingLayoutApply>();
        app.init_resource::<PendingStrategySnapshotRestore>();
        app.add_message::<PanelSpawnRequested>();
        app.add_message::<UndoRedoApplied>();
        app.add_systems(Update, apply_pending_app_edits_system);

        let region_key = "region_001".to_string();
        let new_text = "def strategy(): pass".to_string();

        app.world_mut().spawn((
            WindowRoot,
            StrategyEditorId {
                region_key: region_key.clone(),
            },
            StrategyFragment {
                source: "".to_string(),
                dirty: false,
            },
        ));

        {
            let mut history = app.world_mut().resource_mut::<AppHistory>();
            history
                .pending
                .queue
                .push_back(AppEditAction::SetStrategySource {
                    region_key: region_key.clone(),
                    text: new_text.clone(),
                });
        }

        app.update();

        let auto_save = app.world().resource::<StrategyAutoSaveState>();
        assert!(auto_save.dirty);
        assert!(auto_save.last_change.is_some());
    }

    #[test]
    fn apply_strategy_snapshot_restore_sets_autosave_dirty() {
        use crate::ui::components::WindowRoot;
        let mut app = App::new();
        app.init_resource::<StrategyBuffer>();
        app.init_resource::<AppHistory>();
        app.init_resource::<StrategyAutoSaveState>();
        app.init_resource::<PendingStrategySnapshotRestore>();
        app.add_message::<UndoRedoApplied>();
        app.add_systems(Update, apply_strategy_snapshot_restore_system);

        app.world_mut().spawn(StrategyEditorRoot);

        let region_key = "region_001".to_string();
        let snapshot_text = "restored_source = 123".to_string();

        app.world_mut().spawn((
            WindowRoot,
            StrategyEditorId {
                region_key: region_key.clone(),
            },
            StrategyFragment {
                source: "".to_string(),
                dirty: false,
            },
        ));

        {
            let mut pending = app
                .world_mut()
                .resource_mut::<PendingStrategySnapshotRestore>();
            pending.snapshot = Some((region_key.clone(), snapshot_text.clone()));
        }

        app.update();

        let auto_save = app.world().resource::<StrategyAutoSaveState>();
        assert!(auto_save.dirty);
        assert!(auto_save.last_change.is_some());

        let pending = app.world().resource::<PendingStrategySnapshotRestore>();
        assert!(pending.snapshot.is_none());
    }

    #[test]
    fn debounced_autosave_system_flushes_when_debounce_elapsed() {
        use crate::ui::components::WindowRoot;
        let mut app = App::new();
        app.init_resource::<StrategyBuffer>();
        app.init_resource::<StrategyAutoSaveState>();
        app.add_systems(Update, debounced_strategy_autosave_system);

        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("strategy.py");

        app.world_mut().spawn((
            WindowRoot,
            StrategyEditorId {
                region_key: "region_001".to_string(),
            },
            StrategyFragment {
                source: "x = 1".to_string(),
                dirty: true,
            },
        ));

        {
            let mut buffer = app.world_mut().resource_mut::<StrategyBuffer>();
            buffer.cache_path = Some(cache_path.clone());
        }
        {
            let mut auto_save = app.world_mut().resource_mut::<StrategyAutoSaveState>();
            auto_save.dirty = true;
            auto_save.last_change = Some(Instant::now() - Duration::from_secs(2));
        }

        app.update();

        assert!(cache_path.exists());
        let written = fs::read_to_string(&cache_path).unwrap();
        assert!(written.contains("x = 1"), "written: {}", written);

        let auto_save = app.world().resource::<StrategyAutoSaveState>();
        assert!(!auto_save.dirty);
        assert!(auto_save.last_change.is_none());
    }

    #[test]
    fn debounced_autosave_system_clears_fragment_dirty_after_flush() {
        // Medium fix: autosave 成功時は fragment.dirty も false にしないと
        // menu_bar の dirty_count が 0 にならず "*" 表示が残る。
        use crate::ui::components::WindowRoot;
        let mut app = App::new();
        app.init_resource::<StrategyBuffer>();
        app.init_resource::<StrategyAutoSaveState>();
        app.add_systems(Update, debounced_strategy_autosave_system);

        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("strategy.py");

        let entity = app
            .world_mut()
            .spawn((
                WindowRoot,
                StrategyEditorId {
                    region_key: "region_001".to_string(),
                },
                StrategyFragment {
                    source: "x = 1".to_string(),
                    dirty: true,
                },
            ))
            .id();

        {
            let mut buffer = app.world_mut().resource_mut::<StrategyBuffer>();
            buffer.cache_path = Some(cache_path.clone());
        }
        {
            let mut auto_save = app.world_mut().resource_mut::<StrategyAutoSaveState>();
            auto_save.dirty = true;
            auto_save.last_change = Some(Instant::now() - Duration::from_secs(2));
        }

        app.update();

        let fragment = app.world().get::<StrategyFragment>(entity).unwrap();
        assert!(
            !fragment.dirty,
            "fragment.dirty should be cleared after autosave flush"
        );
    }

    #[test]
    fn debounced_autosave_system_skips_when_within_debounce() {
        use crate::ui::components::WindowRoot;
        let mut app = App::new();
        app.init_resource::<StrategyBuffer>();
        app.init_resource::<StrategyAutoSaveState>();
        app.add_systems(Update, debounced_strategy_autosave_system);

        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("strategy.py");

        app.world_mut().spawn((
            WindowRoot,
            StrategyEditorId {
                region_key: "region_001".to_string(),
            },
            StrategyFragment {
                source: "x = 1".to_string(),
                dirty: true,
            },
        ));

        {
            let mut buffer = app.world_mut().resource_mut::<StrategyBuffer>();
            buffer.cache_path = Some(cache_path.clone());
        }
        {
            let mut auto_save = app.world_mut().resource_mut::<StrategyAutoSaveState>();
            auto_save.dirty = true;
            auto_save.last_change = Some(Instant::now());
        }

        app.update();

        assert!(!cache_path.exists());

        let auto_save = app.world().resource::<StrategyAutoSaveState>();
        assert!(auto_save.dirty);
        assert!(auto_save.last_change.is_some());
    }

    /// 退避マーカーが layout 権威を保持することの回帰: 最初から Hidden のウィンドウは
    /// Replay→Manual→Replay を経ても Hidden のまま（blanket-Inherited にしない）。
    #[test]
    fn mode_hidden_marker_preserves_layout_authority() {
        use bevy::transform::TransformPlugin;

        let mut app = App::new();
        app.add_plugins(TransformPlugin);
        app.init_resource::<crate::trading::ExecutionModeRes>();
        app.add_systems(Update, apply_strategy_editor_mode_visibility_system);

        // layout で visible:false 相当 = 最初から Hidden。
        let window = app
            .world_mut()
            .spawn((WindowRoot, PanelKind::StrategyEditor, Visibility::Hidden))
            .id();

        // Replay: Hidden のまま（触られない）。
        app.update();
        assert_eq!(*app.world().get::<Visibility>(window).unwrap(), Visibility::Hidden);

        // Manual: Hidden を退避して Hidden のまま。
        app.world_mut().resource_mut::<crate::trading::ExecutionModeRes>().mode =
            crate::trading::ExecutionMode::LiveManual;
        app.update();
        assert_eq!(*app.world().get::<Visibility>(window).unwrap(), Visibility::Hidden);

        // Replay へ戻す: 退避値 Hidden に復元 → blanket-Inherited にならない。
        app.world_mut().resource_mut::<crate::trading::ExecutionModeRes>().mode =
            crate::trading::ExecutionMode::Replay;
        app.update();
        assert_eq!(
            *app.world().get::<Visibility>(window).unwrap(),
            Visibility::Hidden,
            "layout が Hidden を意図していたなら Manual を抜けても Hidden のまま"
        );
        assert!(app.world().get::<StrategyEditorModeHidden>(window).is_none());
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Slice 1 (#50): bevscode CodeEditor を Projected Node として spawn / 投影する系統
//
// ADR 0006: cosmic_edit を bevscode に置き換える Phase B の最初のスライス。Slice 6 で cosmic を
// 撤去した後は、本ブロックが唯一の Projected Node 経路となる。
// ─────────────────────────────────────────────────────────────────────────────

use bevscode::prelude::*;
use bevscode::types::GutterTextView;
use bevy::ui::UiGlobalTransform;
use bevy::window::PrimaryWindow;
use bevy_instanced_text::DisplayLayout;
use bevy_tree_sitter::arborium::lang_python;

/// Strategy Editor の world-space root sprite に貼る marker。
/// projection system がこれを起点に world→screen 投影し、`StrategyEditorNode` を追従させる。
#[derive(Component)]
pub struct StrategyEditorRoot;

/// bevscode `CodeEditor` Node entity に貼る、root への back-link + region 識別。
#[derive(Component)]
pub struct StrategyEditorNode {
    pub root: Entity,
    pub region_key: String,
}

/// spawn 直後に seed テキストを bevscode に流すための一回限りマーカー。
/// `apply_pending_strategy_seed_system` が `SetTextRequested` を送って即外す。
#[derive(Component)]
pub struct StrategyEditorPendingSeed(pub String);

/// Projection 用ベース定数。旧 cosmic の `EDITOR_FONT_SIZE` (14.0) /
/// `EDITOR_LINE_HEIGHT` (18.0) と同値にして、scale=1.0 のとき同じ字面になるようにする。
const PROJECTED_BASE_FONT_SIZE: f32 = 14.0;
const PROJECTED_BASE_LINE_HEIGHT: f32 = 18.0;

/// `Added<StrategyEditorRoot>` を watch して bevscode `CodeEditor` peer を spawn する。
///
/// spawn_strategy_editor_panel は `&AssetServer` を持たないため、root の生成だけ済ませて
/// このシステムに peer 生成を委譲する（1 フレーム遅延、Slice 1 では cosmic が描画しているので許容）。
/// seed テキストは root と同じ frame に `StrategyFragment` が立っているので、それを読んで
/// `StrategyEditorPendingSeed` に積んでおく（実際の `SetTextRequested` 送信は次の system で）。
pub fn spawn_bevscode_peer_on_strategy_editor_added(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    roots: Query<(Entity, &StrategyEditorId, &StrategyFragment), Added<StrategyEditorRoot>>,
) {
    for (root_entity, id, fragment) in roots.iter() {
        let font: Handle<Font> = asset_server.load("fonts/FiraMono-Regular.ttf");
        commands.spawn((
            CodeEditor,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                // 初期値はダミー（次フレームの projection system が world rect で上書きする）。
                width: Val::Px(0.0),
                height: Val::Px(0.0),
                ..default()
            },
            TextFont::from_font_size(PROJECTED_BASE_FONT_SIZE).with_font(font),
            MonoFontFaces::default(),
            bevy::text::LineHeight::Px(PROJECTED_BASE_LINE_HEIGHT),
            // Slice 2 (#50): bevscode が input を受ける（cosmic は描画専用に退く）。
            // focus は apply_pending_strategy_seed_system が SetTextRequested 送信と同タイミングで立てる。
            StrategyEditorNode {
                root: root_entity,
                region_key: id.region_key.clone(),
            },
            // Slice 5 (#50): compute_find_match_spans_system は bevscode peer 側で動くように切替済み。
            // spawn 時から FindMatchSpans を貼り、cosmic 側の重複は Slice 6 で撤去予定。
            FindMatchSpans::default(),
            StrategyEditorPendingSeed(fragment.source.clone()),
            Name::new("StrategyEditorNode(bevscode)"),
        ));
    }
}

/// `StrategyEditorPendingSeed` が立っている entity に `SetTextRequested` を送って marker を外す。
/// `Added<TextBuffer<RopeBuffer>>` のような厳密な準備完了 wait は不要（bevscode 側が次フレーム以降で
/// 受け取った text を rope に積む）。送信 → 即 remove で「複数フレーム送り続ける」事故を防ぐ。
///
/// Slice 2 (#50): 同タイミングで `InputFocus` を bevscode entity に設定する。これで cosmic は
/// 描画専用に退き、ユーザー入力は bevscode の `TextBuffer<RopeBuffer>` に流れる
/// （`sync_bevscode_to_strategy_fragment_system` が autosave / AppHistory を駆動）。
pub fn apply_pending_strategy_seed_system(
    mut commands: Commands,
    mut text_writer: MessageWriter<SetTextRequested>,
    mut lang_writer: MessageWriter<SetLanguageRequested>,
    mut input_focus: ResMut<bevy::input_focus::InputFocus>,
    pending: Query<(Entity, &StrategyEditorPendingSeed)>,
) {
    for (entity, seed) in pending.iter() {
        text_writer.write(SetTextRequested {
            entity,
            text: seed.0.clone(),
        });
        // Slice 4 (#50): Python シンタックスハイライトを bevscode/bevy_tree_sitter で有効化。
        // grammar は seed 投入と同タイミングで一度だけ流す。pending マーカーが外れるので二重発火しない。
        lang_writer.write(SetLanguageRequested {
            entity,
            grammar: Some(TreeSitterGrammar::new(
                lang_python::language().into(),
                lang_python::HIGHLIGHTS_QUERY,
            )),
        });
        input_focus.set(entity);
        commands
            .entity(entity)
            .remove::<StrategyEditorPendingSeed>();
    }
}

/// 毎フレーム world rect → screen rect 投影で bevscode `CodeEditor` Node を追従させる。
///
/// 投影規約は ADR 0006 で確立。marker 型:
/// - root: `StrategyEditorRoot`（floating window root に常に貼ってある）
/// - node: `StrategyEditorNode { root, region_key }`（bevscode peer）
///
/// Z=200 ピンは Slice 6 で cosmic 撤去後に有効化したい挙動だが、cosmic と並存中も Bevy UI が
/// 常に world sprite の後に描画されるため、bevscode 側は実害なく前面に来る。z は触らない
/// （他 floating window との z 競合は cosmic 撤去まで現状維持）。
pub fn project_strategy_editor_node_system(
    roots: Query<(&Transform, &Sprite), With<StrategyEditorRoot>>,
    cam_q: Query<(&Transform, &Projection), (With<Camera2d>, Without<StrategyEditorRoot>)>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut editors: Query<
        (
            &StrategyEditorNode,
            &mut Node,
            &mut TextFont,
            &mut bevy::text::LineHeight,
        ),
        Without<StrategyEditorRoot>,
    >,
) {
    let Ok((cam_tf, projection)) = cam_q.single() else {
        return;
    };
    let Projection::Orthographic(ortho) = projection else {
        return;
    };
    let scale = ortho.scale.max(1e-6);
    let Ok(window) = windows.single() else {
        return;
    };
    let win_w = window.width();
    let win_h = window.height();
    let cam = cam_tf.translation.truncate();

    for (marker, mut node, mut font, mut line_height) in editors.iter_mut() {
        let Ok((root_tf, root_sprite)) = roots.get(marker.root) else {
            continue;
        };
        let Some(root_size) = root_sprite.custom_size else {
            continue;
        };

        let root_center = root_tf.translation.truncate();
        let content_w_world = root_size.x;
        let content_h_world = (root_size.y - TITLE_BAR_HEIGHT).max(10.0);
        let content_tl_world_x = root_center.x - root_size.x / 2.0;
        let content_tl_world_y = root_center.y + root_size.y / 2.0 - TITLE_BAR_HEIGHT;

        let node_left = (content_tl_world_x - cam.x) / scale + win_w / 2.0;
        let node_top = -(content_tl_world_y - cam.y) / scale + win_h / 2.0;
        let node_w = content_w_world / scale;
        let node_h = content_h_world / scale;

        let new_font_size = (PROJECTED_BASE_FONT_SIZE / scale).max(1.0);
        let new_line_height_px = (PROJECTED_BASE_LINE_HEIGHT / scale).max(2.0);

        let new_left = Val::Px(node_left);
        if node.left != new_left {
            node.left = new_left;
        }
        let new_top = Val::Px(node_top);
        if node.top != new_top {
            node.top = new_top;
        }
        let new_width = Val::Px(node_w);
        if node.width != new_width {
            node.width = new_width;
        }
        let new_height = Val::Px(node_h);
        if node.height != new_height {
            node.height = new_height;
        }
        if (font.font_size - new_font_size).abs() > 0.01 {
            font.font_size = new_font_size;
        }
        let new_line_height = bevy::text::LineHeight::Px(new_line_height_px);
        if *line_height != new_line_height {
            *line_height = new_line_height;
        }
    }
}

/// drag / pan で editor が動いたとき、bevy_instanced_text の glyph batch を再構築させる。
///
/// `ui_layout_system` は `ComputedNode` の change detection を size 変化でしか立てず、位置変化
/// （`UiGlobalTransform`）では `bevy_instanced_text` の glyph batch キャッシュが効いて editor 本体・
/// 行番号ガターが前フレームの screen 位置に取り残される。`Changed<UiGlobalTransform>` を検知して
/// editor 本体と対応ガターの `DisplayLayout` を `set_changed()` し、batch 再合成を強制する（ADR 0006）。
/// gutter は bevscode の `GutterTextView { editor }` で対応する editor entity に紐付いている。
pub fn touch_strategy_text_layouts_on_position_change(
    editors: Query<Entity, (With<StrategyEditorNode>, Changed<UiGlobalTransform>)>,
    gutters: Query<(Entity, &GutterTextView)>,
    mut layouts: Query<&mut DisplayLayout>,
) {
    for editor_entity in editors.iter() {
        if let Ok(mut dl) = layouts.get_mut(editor_entity) {
            dl.set_changed();
        }
        for (gutter_entity, marker) in gutters.iter() {
            if marker.editor == editor_entity {
                if let Ok(mut dl) = layouts.get_mut(gutter_entity) {
                    dl.set_changed();
                }
            }
        }
    }
}

/// `StrategyEditorRoot` が despawn されたとき、紐付く bevscode peer も一緒に despawn する。
/// 既存 close ボタン / mode visibility が root を消したときの後片付け。
pub fn cleanup_strategy_editor_node_on_root_despawn(
    mut removed: RemovedComponents<StrategyEditorRoot>,
    nodes: Query<(Entity, &StrategyEditorNode)>,
    mut commands: Commands,
) {
    for root_entity in removed.read() {
        for (node_entity, marker) in nodes.iter() {
            if marker.root == root_entity {
                if let Ok(mut ec) = commands.get_entity(node_entity) {
                    ec.despawn();
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Slice 2 (#50): bevscode ↔ StrategyBuffer 同期 + AppHistory undo bridge
//
// 役割:
// - bevscode の TextBuffer<RopeBuffer> が変わったら `StrategyFragment` に書き戻し autosave を駆動
// - AppHistory が `SetStrategySource` を writeback するとき bevscode entity にも `SetTextRequested` を送る
// - File load / Undo の cosmic 経路（`sync_strategy_buffer_to_editor_system`）も同様に bevscode へ伝搬
// - bevscode 内蔵の Ctrl+Z / Ctrl+Y を無効化（AppHistory が undo を担う設計）
// - echo suppression を共有（cosmic と同じ `AppHistory.suppress_echo_target` を honor）
// ─────────────────────────────────────────────────────────────────────────────

use bevscode::input::EditorAction;
use bevscode::input::keybindings::default_input_map;
use bevscode::plugin::EditorInputManager;

/// Startup で bevscode 既定の EditorInputManager より先に独自 InputMap を spawn する。
/// `Ctrl+Z` / `Ctrl+Y` / `Ctrl+Shift+Z` を bevscode から剥がし、`AppHistory.undo_redo_system` が
/// 唯一の undo/redo 経路になるよう一本化する。bevscode は PostStartup の `spawn_default_input_manager` で
/// 「既に EditorInputManager 持ち entity があればスキップ」する設計なので、Startup で先取りすればよい。
///
/// `Find` 系は Slice 5 で独自実装の find UI を維持するため、暫定で `Find` 系 action もこの段階で剥がす
/// （重複起動を防ぐ）。完全に剥がすかは Slice 5 着手時に再判断。
pub fn install_strategy_editor_keybindings(mut commands: Commands) {
    let mut input_map = default_input_map();
    input_map.clear_action(&EditorAction::Undo);
    input_map.clear_action(&EditorAction::Redo);
    commands.spawn((
        EditorInputManager,
        input_map,
        Name::new("StrategyEditorInputManager"),
    ));
}

/// bevscode `CodeEditor` でユーザーが編集した内容を `StrategyFragment.source` に書き戻し、
/// autosave / AppHistory を駆動する（片側同期: bevscode → StrategyFragment）。
///
/// `sync_editor_to_strategy_buffer_system` の cosmic 版と同じ役割。`Changed<TextBuffer<RopeBuffer>>`
/// を起点に bevscode 側の最新テキストを文字列化し、`suppress_echo_target` と一致すれば echo を抑制する。
/// 一致しなければ `mark_fragment_dirty` で fragment + autosave を更新し、replaying 中でなければ
/// `AppHistory.push_text` で undo 履歴に積む。
pub fn sync_bevscode_to_strategy_fragment_system(
    editors: Query<
        (&StrategyEditorNode, &TextBuffer<RopeBuffer>),
        Changed<TextBuffer<RopeBuffer>>,
    >,
    mut fragments_q: Query<(&StrategyEditorId, &mut StrategyFragment), With<WindowRoot>>,
    mut history: ResMut<AppHistory>,
    mut auto_save: ResMut<StrategyAutoSaveState>,
) {
    for (marker, buffer) in editors.iter() {
        let new_text = buffer.chars().collect::<String>();
        let region_key = marker.region_key.clone();

        let Some((_, mut fragment)) = fragments_q
            .iter_mut()
            .find(|(id, _)| id.region_key == region_key)
        else {
            warn!(
                "bevscode TextBuffer changed for region '{}' but no matching WindowRoot",
                region_key
            );
            continue;
        };

        if let Some((target_key, target_text)) = history.suppress_echo_target.clone() {
            if target_key == region_key && target_text.as_str() == new_text.as_str() {
                history.suppress_echo_target = None;
                fragment.source = new_text;
                continue;
            } else {
                history.suppress_echo_target = None;
            }
        }
        if fragment.source == new_text {
            continue;
        }
        if !history.is_replaying() {
            history.push_text(
                region_key.clone(),
                fragment.source.clone(),
                new_text.clone(),
            );
        }
        mark_fragment_dirty(&mut fragment, &mut auto_save, new_text);
    }
}

/// AppHistory が `SetStrategySource` を writeback したり、`StrategyFileLoadRequested` で外部 .py を
/// 流し込んだりした直後に、bevscode 側 entity にも `SetTextRequested` を送って TextBuffer を揃える。
///
/// 検知は `Changed<StrategyFragment>` で行い、`AppHistory.suppress_echo_target` が立っているなら
/// それが今回の writeback 起源と分かるので即時 SetTextRequested を流す。echo suppression は
/// `sync_bevscode_to_strategy_fragment_system` 側で受け止めるので無限ループは起きない。
///
/// 注: cosmic 経路の `sync_strategy_buffer_to_editor_system` は UndoRedoApplied / FileLoad 駆動だが、
/// bevscode は `Changed<StrategyFragment>` の方が来た契機を一発で拾えるので採用。
pub fn sync_strategy_fragment_to_bevscode_system(
    fragments_q: Query<
        (&StrategyEditorId, &StrategyFragment),
        (With<WindowRoot>, Changed<StrategyFragment>),
    >,
    editors: Query<(Entity, &StrategyEditorNode, &TextBuffer<RopeBuffer>)>,
    mut writer: MessageWriter<SetTextRequested>,
) {
    for (id, fragment) in fragments_q.iter() {
        for (editor_entity, marker, buffer) in editors.iter() {
            if marker.region_key != id.region_key {
                continue;
            }
            // すでに bevscode 側が同じ内容なら send 不要（無駄な Changed を立てない）。
            let current = buffer.chars().collect::<String>();
            if current == fragment.source {
                continue;
            }
            writer.write(SetTextRequested {
                entity: editor_entity,
                text: fragment.source.clone(),
            });
        }
    }
}
