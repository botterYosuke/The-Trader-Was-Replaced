use crate::ui::components::{
    OpenStrategyRequested, PanelKind, PanelSpawnRequested, PanelSpawnSource, RedoMenuRequested,
    StrategyBuffer, UndoMenuRequested, WindowRoot,
};
use crate::ui::editor_history::{
    AppEditAction, AppHistory, PendingStrategySnapshotRestore, UndoRedoApplied,
};
use crate::ui::floating_window::{FloatingWindowSpec, spawn_floating_window};
use crate::ui::layout_persistence::PendingLayoutApply;
use bevy::prelude::*;
use bevy_cosmic_edit::cosmic_text::{Attrs, AttrsOwned, Edit, Metrics, Shaping};
use bevy_cosmic_edit::{
    CosmicBackgroundColor, CosmicFontSystem, CosmicRenderScale, CosmicTextAlign, CursorColor,
    ScrollEnabled,
};
use bevy_cosmic_edit::{CosmicTextChanged, prelude::*};

// ── Bevy native 版 Strategy Editor ─────────────

const PANEL_SIZE: Vec2 = Vec2::new(500.0, 400.0);
const PANEL_POSITION: Vec2 = Vec2::new(-300.0, 50.0);
const EDITOR_SIZE: Vec2 = Vec2::new(440.0, 320.0);
const EDITOR_FONT_SIZE: f32 = 14.0;
const EDITOR_LINE_HEIGHT: f32 = 18.0;
const EDITOR_MAX_SUPERSAMPLE: f32 = 4.0;
const ACCENT: Color = Color::srgba(0.63, 0.44, 1.0, 0.4); // SVG #a070ff (purple)
const EDITOR_BG: Color = Color::srgba(0.02, 0.02, 0.04, 1.0);

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
    buffer: &mut StrategyBuffer,
    auto_save: &mut StrategyAutoSaveState,
) -> std::io::Result<bool> {
    let Some(path) = buffer.cache_path.as_ref() else {
        return Ok(false);
    };
    std::fs::write(path, &buffer.source)?;
    buffer.dirty = false;
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
fn mark_strategy_dirty(
    buffer: &mut StrategyBuffer,
    auto_save: &mut StrategyAutoSaveState,
    new_source: String,
) {
    buffer.source = new_source;
    buffer.dirty = true;
    auto_save.dirty = true;
    auto_save.last_change = Some(std::time::Instant::now());
}

/// エディタ本体（TextEdit2d 付き sprite）を識別するマーカー。
/// Sub-step 1.8c で `Query<&mut CosmicEditBuffer, With<StrategyEditorContent>>` で取りに行く。
#[derive(Component)]
pub struct StrategyEditorContent;

/// Tracks zoom state for the strategy editor to drive `CosmicRenderScale`.
#[derive(Component)]
pub struct ZoomResponsiveEditor {
    max_supersample: f32,
    last_supersample: f32,
}

/// dispatcher から呼ばれる spawn 関数。
pub fn spawn_strategy_editor_panel(commands: &mut Commands, font_system: &mut CosmicFontSystem) {
    let (root, content_area, title_bar) = spawn_floating_window(
        commands,
        FloatingWindowSpec {
            title: "STRATEGY EDITOR".to_string(),
            size: PANEL_SIZE,
            position: PANEL_POSITION,
            accent: ACCENT,
        },
    );
    commands.entity(root).insert(PanelKind::StrategyEditor);

    // bevy_cosmic_edit の TextEdit2d。Sprite + CosmicEditBuffer は自動で required components として付く。
    let editor = commands
        .spawn((
            TextEdit2d,
            Sprite {
                custom_size: Some(EDITOR_SIZE),
                color: Color::WHITE,
                ..default()
            },
            CosmicEditBuffer::new(
                font_system,
                Metrics::new(EDITOR_FONT_SIZE, EDITOR_LINE_HEIGHT),
            )
            .with_text(
                font_system,
                "// strategy code\n",
                Attrs::new().color(CosmicColor::rgb(220, 220, 220)),
            ),
            DefaultAttrs(AttrsOwned::new(
                Attrs::new().color(CosmicColor::rgb(220, 220, 220)),
            )),
            CursorColor(Color::WHITE),
            CosmicBackgroundColor(EDITOR_BG),
            Transform::from_xyz(0.0, 0.0, 0.1),
            StrategyEditorContent,
            ZoomResponsiveEditor {
                max_supersample: EDITOR_MAX_SUPERSAMPLE,
                last_supersample: 1.0,
            },
            CosmicRenderScale(1.0),
            // コードエディタ用途では default の Center align だと表示が不安定なため TopLeft を明示。
            CosmicTextAlign::TopLeft { padding: 8 },
            // スクロールはデフォルト無効。camera.rs の pancam_suppression_over_editor_system が
            // 「カーソルがエディタ上 かつ Ctrl 非押下」のフレームだけ Enabled に切り替える。
            // TextEdit2d は ScrollEnabled を required component に含めないため、ここで明示付与しないと
            // cosmic_edit の input_mouse が editor entity を丸ごとスキップし、スクロール切替が一切効かない。
            ScrollEnabled::Disabled,
        ))
        .id();

    commands.entity(content_area).add_child(editor);
    commands.insert_resource(FocusedWidget(Some(editor)));

    // `title_bar` は floating_window 側で × ボタンを持つので追加配置はしない。
    let _ = title_bar;
}

pub fn update_strategy_editor_zoom_system(
    camera_q: Query<&OrthographicProjection, With<Camera2d>>,
    mut editor_q: Query<(&mut ZoomResponsiveEditor, &mut CosmicRenderScale)>,
    mut last_camera_scale: Local<f32>,
) {
    let Ok(projection) = camera_q.get_single() else {
        return;
    };

    let camera_scale = projection.scale.max(0.01);

    // Skip entirely when camera scale is stable and no editors exist.
    // When editors exist we always iterate — the last_supersample guard inside the loop
    // prevents redundant CosmicRenderScale mutations, which is important so newly-spawned
    // editors (last_supersample = 1.0) get the correct scale on the very first frame
    // even if the camera hasn't moved since the editor was added.
    if editor_q.is_empty() && (*last_camera_scale - camera_scale).abs() < 0.001 {
        return;
    }
    *last_camera_scale = camera_scale;

    for (mut responsive, mut render_scale) in &mut editor_q {
        let supersample = (1.0 / camera_scale).clamp(1.0, responsive.max_supersample);
        if (responsive.last_supersample - supersample).abs() < 0.01 {
            continue;
        }
        responsive.last_supersample = supersample;
        render_scale.0 = supersample;
    }
}

/// `OpenStrategyRequested` イベント（ファイル → buffer に丸ごとロード）の直後に、
/// cosmic_edit エディタの内容を `buffer.source` で置き換える（片側同期: buffer → editor）。
///
/// 旧実装は `buffer.is_changed()` でトリガしていたが、`sync_editor_to_strategy_buffer_system`
/// がユーザー入力ごとに `buffer.source = new_text` を書く（DerefMut で次フレーム is_changed = true）
/// → buffer→editor 同期が走り `set_text` でカーソルが先頭にリセット、という不具合があった。
/// イベント駆動に切り替えることで「外部から `.py` を読み込んだ瞬間」だけに発火範囲を絞る。
///
/// system 順序: `open_strategy_buffer_system` が同じイベントを読んで `buffer.source` を
/// 更新するので、本 system は必ず `.after(open_strategy_buffer_system)` で走らせる。
/// `EventReader` は system ごとに独立した読み取りカーソルを持つため、両方とも同じイベントを読める。
/// buffer.source の内容を cosmic_edit エディタに反映するヘルパー。
fn apply_buffer_to_editor(
    source: &str,
    font_system: &mut CosmicFontSystem,
    editor_q: &mut Query<
        (&mut CosmicEditBuffer, Option<&mut CosmicEditor>),
        With<StrategyEditorContent>,
    >,
) {
    for (mut edit_buffer, editor_opt) in editor_q.iter_mut() {
        edit_buffer.set_text(font_system, source, Attrs::new());
        if let Some(mut editor) = editor_opt {
            editor.with_buffer_mut(|b| {
                b.set_text(font_system, source, Attrs::new(), Shaping::Advanced);
                b.set_redraw(true);
            });
        }
    }
}

pub fn sync_strategy_buffer_to_editor_system(
    mut open_events: EventReader<OpenStrategyRequested>,
    mut undo_events: EventReader<UndoRedoApplied>,
    buffer: Res<StrategyBuffer>,
    mut font_system: ResMut<CosmicFontSystem>,
    mut editor_q: Query<
        (&mut CosmicEditBuffer, Option<&mut CosmicEditor>),
        With<StrategyEditorContent>,
    >,
) {
    let has_open = !open_events.is_empty();
    let has_undo = !undo_events.is_empty();
    open_events.clear();
    undo_events.clear();

    if !has_open && !has_undo {
        return;
    }

    if has_open && buffer.original_path.is_none() {
        return;
    }

    apply_buffer_to_editor(&buffer.source, &mut font_system, &mut editor_q);
}

/// cosmic_edit エディタでユーザーが編集した内容を `StrategyBuffer.source` に書き戻し、
/// `dirty = true` を立てる（片側同期: editor → buffer）。
///
/// `CosmicTextChanged` イベントは bevy_cosmic_edit の input system
/// （キーボード入力 / paste / drop）で発火する。`CosmicEditBuffer::set_text`
/// からは発火しないので、buffer → editor 同期（`sync_strategy_buffer_to_editor_system`）
/// とのループは発生しない（exact version 0.26.0 の input.rs / buffer.rs で確認済）。
///
/// イベント本体は `CosmicTextChanged(pub (Entity, String))` というタプル struct。
/// 第 1 要素が編集されたエディタ entity、第 2 要素が新しい全文。
/// Strategy Editor 以外のエディタ entity からのイベントは無視する。
pub fn sync_editor_to_strategy_buffer_system(
    mut events: EventReader<CosmicTextChanged>,
    editor_q: Query<Entity, With<StrategyEditorContent>>,
    mut buffer: ResMut<StrategyBuffer>,
    mut history: ResMut<AppHistory>,
    mut auto_save: ResMut<StrategyAutoSaveState>,
) {
    for CosmicTextChanged((entity, new_text)) in events.read() {
        if !editor_q.contains(*entity) {
            continue;
        }
        // suppress_echo_target が Some のとき: undo/redo 適用直後に cosmic_edit が
        // 返す echo を「期待テキスト一致」で判別して無視する。
        // - new_text が target と一致 → echo として消費・無視（buffer.source を同期して continue）
        // - 一致しない → ターゲットをクリアして通常入力として処理に流す
        if let Some(ref target) = history.suppress_echo_target.clone() {
            if new_text.as_str() == target.as_str() {
                history.suppress_echo_target = None;
                buffer.source = new_text.clone();
                continue;
            } else {
                // 違うテキストが来たらターゲットをクリアして通常処理
                history.suppress_echo_target = None;
            }
        }
        if buffer.source == *new_text {
            continue;
        }
        // is_replaying 中でなければ Undo 履歴に記録する
        if !history.is_replaying() {
            history.push_text(buffer.source.clone(), new_text.clone());
        }
        mark_strategy_dirty(&mut buffer, &mut auto_save, new_text.clone());
    }
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
    mut undo_menu_ev: EventReader<UndoMenuRequested>,
    mut redo_menu_ev: EventReader<RedoMenuRequested>,
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
            && (keys.just_pressed(KeyCode::KeyY)
                || (keys.just_pressed(KeyCode::KeyZ) && shift)));

    if do_undo {
        history.replaying_depth += 1;
        let changed = {
            let AppHistory { record, pending, .. } = &mut *history;
            record.undo(pending).is_some()
        };
        if !changed {
            history.replaying_depth -= 1; // 何も起きなかったので即戻す
        }
        *cooldown = 0.05;
    } else if do_redo {
        history.replaying_depth += 1;
        let changed = {
            let AppHistory { record, pending, .. } = &mut *history;
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
    mut buffer: ResMut<StrategyBuffer>,
    mut windows_q: Query<(Entity, &PanelKind, &mut Transform), With<WindowRoot>>,
    mut commands: Commands,
    mut spawn_ev: EventWriter<PanelSpawnRequested>,
    mut pending_layout: ResMut<PendingLayoutApply>,
    mut pending_restore: ResMut<PendingStrategySnapshotRestore>,
    mut undo_applied: EventWriter<UndoRedoApplied>,
    mut auto_save: ResMut<StrategyAutoSaveState>,
) {
    if history.pending.queue.is_empty() {
        return;
    }

    let mut any_text = false; // テキスト変更があったかのフラグ
    let actions: Vec<_> = history.pending.queue.drain(..).collect();
    for action in actions {
        match action {
            AppEditAction::SetStrategySource { text } => {
                // buffer に書き込む text が echo で返ってくるテキストなので、
                // そのテキストを target としてセットし期待一致方式で echo を抑制する。
                history.suppress_echo(text.clone());
                mark_strategy_dirty(&mut buffer, &mut auto_save, text);
                any_text = true;
            }
            AppEditAction::MoveWindow { kind, position } => {
                // Entity の代わりに PanelKind でパネルを検索
                if let Some((_, _, mut tf)) = windows_q.iter_mut().find(|(_, k, _)| **k == kind) {
                    tf.translation.x = position.x;
                    tf.translation.y = position.y;
                }
            }
            AppEditAction::SpawnWindow { layout, strategy_snapshot } => {
                spawn_ev.send(PanelSpawnRequested {
                    kind: layout.kind,
                    source: PanelSpawnSource::UndoRedo,
                });
                // 翌フレームで位置・サイズ・z を復元（apply_pending_layout_system が処理）
                pending_layout.windows.push(layout.clone());
                // Strategy Editor の場合はアクションが持つ snapshot を復元
                if layout.kind == PanelKind::StrategyEditor
                    && let Some(snap) = strategy_snapshot
                {
                    pending_restore.snapshot = Some(snap);
                }
            }
            AppEditAction::DespawnWindow { kind } => {
                if let Some((entity, _, _)) = windows_q.iter().find(|(_, k, _)| **k == kind) {
                    commands.entity(entity).despawn_recursive();
                }
            }
        }
    }

    // テキスト変更があり かつ replaying 中のときのみ UndoRedoApplied を送る
    // （is_replaying チェックは replaying_depth -= 1 の前に行う）
    if any_text && history.is_replaying() {
        undo_applied.send(UndoRedoApplied);
    }

    // drain 完了後に replaying_depth をデクリメント（0 以下にはしない）
    if history.replaying_depth > 0 {
        history.replaying_depth -= 1;
    }
}

/// `PendingStrategySnapshotRestore` にスナップショットが積まれていたら
/// buffer.source を復元し、エディタに反映するトリガーとして `UndoRedoApplied` を発火する。
/// StrategyEditorContent entity が生成されるまで待つ（2 段階遅延）。
pub fn apply_strategy_snapshot_restore_system(
    mut pending_restore: ResMut<PendingStrategySnapshotRestore>,
    mut buffer: ResMut<StrategyBuffer>,
    mut history: ResMut<AppHistory>,
    editor_q: Query<Entity, With<StrategyEditorContent>>,
    mut undo_applied: EventWriter<UndoRedoApplied>,
    mut auto_save: ResMut<StrategyAutoSaveState>,
) {
    if pending_restore.snapshot.is_none() {
        return;
    }
    // StrategyEditor entity が生成されるまで待つ
    if editor_q.is_empty() {
        return;
    }
    if let Some(snapshot) = pending_restore.snapshot.take() {
        // buffer に書き込む snapshot が echo で返ってくるテキストなので、
        // そのテキストを target としてセットし期待一致方式で echo を抑制する。
        history.suppress_echo(snapshot.clone());
        mark_strategy_dirty(&mut buffer, &mut auto_save, snapshot);
        undo_applied.send(UndoRedoApplied);
    }
}

/// 1 秒 debounce で `StrategyBuffer` を `cache_path` に自動保存する system。
///
/// 毎フレーム `should_flush` で経過時間を判定し、満たしたときだけ `flush_strategy_cache` を呼ぶ。
/// `cache_path` 未設定 (`Ok(false)`) のときは debounce タイマーをクリアして無限ループを防ぐ。
/// I/O 失敗時は state を保持し、次の debounce 経過で再試行する。
pub fn debounced_strategy_autosave_system(
    mut buffer: ResMut<StrategyBuffer>,
    mut auto_save: ResMut<StrategyAutoSaveState>,
) {
    const DEBOUNCE: std::time::Duration = std::time::Duration::from_secs(1);
    if !should_flush(&auto_save, std::time::Instant::now(), DEBOUNCE) {
        return;
    }
    match flush_strategy_cache(&mut buffer, &mut auto_save) {
        Ok(true) => info!("strategy cache autosaved: {:?}", buffer.cache_path),
        Ok(false) => {
            auto_save.dirty = false;
            auto_save.last_change = None;
        }
        Err(e) => error!("strategy cache autosave failed: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    #[test]
    fn flush_returns_false_when_cache_path_is_none() {
        let mut buffer = StrategyBuffer {
            original_path: None,
            cache_path: None,
            source: "fn main() {}".to_string(),
            dirty: true,
        };
        let mut auto_save = StrategyAutoSaveState {
            dirty: true,
            last_change: Some(Instant::now()),
        };

        let result = flush_strategy_cache(&mut buffer, &mut auto_save);

        assert!(matches!(result, Ok(false)));
        assert!(buffer.dirty);
        assert!(auto_save.dirty);
        assert!(auto_save.last_change.is_some());
    }

    #[test]
    fn flush_writes_file_and_clears_state() {
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("strategy.rs");

        let mut buffer = StrategyBuffer {
            original_path: None,
            cache_path: Some(cache_path.clone()),
            source: "fn main() { println!(\"hello\"); }".to_string(),
            dirty: true,
        };
        let mut auto_save = StrategyAutoSaveState {
            dirty: true,
            last_change: Some(Instant::now()),
        };

        let result = flush_strategy_cache(&mut buffer, &mut auto_save);

        assert_eq!(result.unwrap(), true);
        assert!(cache_path.exists());
        let written = fs::read_to_string(&cache_path).unwrap();
        assert_eq!(written, "fn main() { println!(\"hello\"); }");
        assert!(!buffer.dirty);
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
            source: "fn main() {}".to_string(),
            dirty: true,
        };
        let now = Instant::now();
        let mut auto_save = StrategyAutoSaveState {
            dirty: true,
            last_change: Some(now),
        };

        let result = flush_strategy_cache(&mut buffer, &mut auto_save);

        assert!(result.is_err());
        assert!(buffer.dirty);
        assert!(auto_save.dirty);
        assert_eq!(auto_save.last_change, Some(now));
    }

    #[test]
    fn should_flush_false_when_not_dirty() {
        let state = StrategyAutoSaveState {
            dirty: false,
            last_change: Some(Instant::now()),
        };
        assert!(!should_flush(&state, Instant::now(), Duration::from_secs(1)));
    }

    #[test]
    fn should_flush_false_when_last_change_none() {
        let state = StrategyAutoSaveState {
            dirty: true,
            last_change: None,
        };
        assert!(!should_flush(&state, Instant::now(), Duration::from_secs(1)));
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
    fn mark_strategy_dirty_updates_state() {
        let mut buffer = StrategyBuffer {
            original_path: None,
            cache_path: None,
            source: "old source".to_string(),
            dirty: false,
        };
        let mut auto_save = StrategyAutoSaveState {
            dirty: false,
            last_change: None,
        };

        let new_source = "new source code".to_string();
        mark_strategy_dirty(&mut buffer, &mut auto_save, new_source.clone());

        assert_eq!(buffer.source, new_source);
        assert!(buffer.dirty);
        assert!(auto_save.dirty);
        assert!(auto_save.last_change.is_some());
    }

    #[test]
    fn apply_pending_app_edits_sets_autosave_dirty_on_strategy_source_action() {
        let mut app = App::new();
        app.init_resource::<StrategyBuffer>();
        app.init_resource::<AppHistory>();
        app.init_resource::<StrategyAutoSaveState>();
        app.init_resource::<PendingLayoutApply>();
        app.init_resource::<PendingStrategySnapshotRestore>();
        app.add_event::<PanelSpawnRequested>();
        app.add_event::<UndoRedoApplied>();
        app.add_systems(Update, apply_pending_app_edits_system);

        let new_text = "def strategy(): pass".to_string();
        {
            let mut history = app.world_mut().resource_mut::<AppHistory>();
            history
                .pending
                .queue
                .push_back(AppEditAction::SetStrategySource {
                    text: new_text.clone(),
                });
        }

        app.update();

        let auto_save = app.world().resource::<StrategyAutoSaveState>();
        assert!(auto_save.dirty);
        assert!(auto_save.last_change.is_some());

        let buffer = app.world().resource::<StrategyBuffer>();
        assert!(buffer.dirty);
        assert_eq!(buffer.source, new_text);
    }

    #[test]
    fn apply_strategy_snapshot_restore_sets_autosave_dirty() {
        let mut app = App::new();
        app.init_resource::<StrategyBuffer>();
        app.init_resource::<AppHistory>();
        app.init_resource::<StrategyAutoSaveState>();
        app.init_resource::<PendingStrategySnapshotRestore>();
        app.add_event::<UndoRedoApplied>();
        app.add_systems(Update, apply_strategy_snapshot_restore_system);

        // system の `if editor_q.is_empty() { return; }` を通過させるため、
        // StrategyEditorContent マーカーを持つ entity を 1 個 spawn する。
        app.world_mut().spawn(StrategyEditorContent);

        let snapshot_text = "restored_source = 123".to_string();
        {
            let mut pending = app
                .world_mut()
                .resource_mut::<PendingStrategySnapshotRestore>();
            pending.snapshot = Some(snapshot_text.clone());
        }

        app.update();

        let auto_save = app.world().resource::<StrategyAutoSaveState>();
        assert!(auto_save.dirty);
        assert!(auto_save.last_change.is_some());

        let buffer = app.world().resource::<StrategyBuffer>();
        assert!(buffer.dirty);
        assert_eq!(buffer.source, snapshot_text);

        let pending = app
            .world()
            .resource::<PendingStrategySnapshotRestore>();
        assert!(pending.snapshot.is_none());
    }

    #[test]
    fn debounced_autosave_system_flushes_when_debounce_elapsed() {
        let mut app = App::new();
        app.init_resource::<StrategyBuffer>();
        app.init_resource::<StrategyAutoSaveState>();
        app.add_systems(Update, debounced_strategy_autosave_system);

        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("strategy.py");

        {
            let mut buffer = app.world_mut().resource_mut::<StrategyBuffer>();
            buffer.source = "x = 1".to_string();
            buffer.cache_path = Some(cache_path.clone());
            buffer.dirty = true;
        }
        {
            let mut auto_save = app.world_mut().resource_mut::<StrategyAutoSaveState>();
            auto_save.dirty = true;
            // debounce(1s) を確実に超える 2 秒前を last_change にする
            auto_save.last_change = Some(Instant::now() - Duration::from_secs(2));
        }

        app.update();

        assert!(cache_path.exists());
        assert_eq!(fs::read_to_string(&cache_path).unwrap(), "x = 1");

        let buffer = app.world().resource::<StrategyBuffer>();
        let auto_save = app.world().resource::<StrategyAutoSaveState>();
        assert!(!buffer.dirty);
        assert!(!auto_save.dirty);
        assert!(auto_save.last_change.is_none());
    }

    #[test]
    fn debounced_autosave_system_skips_when_within_debounce() {
        let mut app = App::new();
        app.init_resource::<StrategyBuffer>();
        app.init_resource::<StrategyAutoSaveState>();
        app.add_systems(Update, debounced_strategy_autosave_system);

        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("strategy.py");

        {
            let mut buffer = app.world_mut().resource_mut::<StrategyBuffer>();
            buffer.source = "x = 1".to_string();
            buffer.cache_path = Some(cache_path.clone());
            buffer.dirty = true;
        }
        {
            let mut auto_save = app.world_mut().resource_mut::<StrategyAutoSaveState>();
            auto_save.dirty = true;
            // 経過 0s で debounce 未満
            auto_save.last_change = Some(Instant::now());
        }

        app.update();

        assert!(!cache_path.exists());

        let buffer = app.world().resource::<StrategyBuffer>();
        let auto_save = app.world().resource::<StrategyAutoSaveState>();
        assert!(buffer.dirty);
        assert!(auto_save.dirty);
        assert!(auto_save.last_change.is_some());
    }
}
