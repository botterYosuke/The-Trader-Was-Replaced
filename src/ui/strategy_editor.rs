use crate::trading::{LastRunResult, RunState};
use crate::ui::components::{
    OpenStrategyRequested, PanelKind, PanelSpawnRequested, PanelSpawnSource, StrategyBuffer,
    StrategyRunRequested, WindowRoot,
};
use crate::ui::editor_history::{
    AppEditAction, AppHistory, PendingStrategySnapshotRestore, UndoRedoApplied,
};
use crate::ui::floating_window::{FloatingWindowSpec, spawn_floating_window};
use crate::ui::layout_persistence::PendingLayoutApply;
use bevy::ecs::system::IntoObserverSystem;
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

const TITLE_BAR_BUTTON_SIZE: Vec2 = Vec2::new(80.0, 24.0);
const TITLE_BAR_BUTTON_GAP: f32 = 8.0;
const TITLE_BAR_BUTTON_Z: f32 = 0.2;
const BUTTON_ENABLED_ALPHA: f32 = 1.0;
const BUTTON_DISABLED_ALPHA: f32 = 0.3;
const SAVE_BUTTON_COLOR: Color = Color::srgba(0.25, 0.55, 0.35, 1.0); // 緑系: 保存
const RUN_BUTTON_COLOR: Color = Color::srgba(0.55, 0.35, 0.75, 1.0); // 紫系: ACCENT 寄り

/// Save Cache ボタン sprite に付けるマーカー。
/// 毎フレーム `StrategyBuffer.dirty` を見て alpha を更新する system で query される。
#[derive(Component)]
pub struct StrategySaveButton;

/// Run ボタン sprite に付けるマーカー。
#[derive(Component)]
pub struct StrategyRunButton;

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

/// タイトルバー上に水平に並べるラベル付きボタン。
/// Strategy Editor の Save Cache / Run は同じ見た目ロジックなので 1 箇所に集約する。
///
/// - `marker`: `StrategySaveButton` か `StrategyRunButton`。後段の system が
///   `Query<&mut Sprite, With<Marker>>` で alpha を更新する目印。
/// - `on_click`: `Trigger<Pointer<Click>>` を取る observer クロージャ。
///   有効/無効判定はクロージャ内で行う（observer subscribe をいじるより単純）。
fn spawn_title_bar_button<Marker, F, B, ObsMarker>(
    commands: &mut Commands,
    title_bar: Entity,
    local_pos: Vec2,
    base_color: Color,
    label: &str,
    marker: Marker,
    on_click: F,
) where
    Marker: Component,
    F: IntoObserverSystem<Pointer<Click>, B, ObsMarker>,
    B: Bundle,
{
    let button = commands
        .spawn((
            Sprite {
                color: base_color,
                custom_size: Some(TITLE_BAR_BUTTON_SIZE),
                ..default()
            },
            Transform::from_xyz(local_pos.x, local_pos.y, TITLE_BAR_BUTTON_Z),
            marker,
        ))
        .observe(on_click)
        .id();

    let text = commands
        .spawn((
            Text2d::new(label.to_string()),
            TextFont {
                font_size: 14.0,
                ..default()
            },
            TextColor(Color::WHITE),
            Transform::from_xyz(0.0, 0.0, 0.01),
        ))
        .id();
    commands.entity(button).add_child(text);
    commands.entity(title_bar).add_child(button);
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

    // ── Save Cache / Run ボタンをタイトルバー右端に積む ───────────────
    // floating_window.rs が右端に × ボタン(20px + margin 8px×2 = 36px) を置くため、
    // タイトルバーボタンはその分だけ左に退避させる。
    const CLOSE_BTN_RESERVED: f32 = 36.0; // 20(btn) + 8(margin_right) + 8(gap)
    let title_bar_right_inner = PANEL_SIZE.x / 2.0
        - TITLE_BAR_BUTTON_SIZE.x / 2.0
        - TITLE_BAR_BUTTON_GAP
        - CLOSE_BTN_RESERVED;
    let run_x = title_bar_right_inner;
    let save_x = run_x - TITLE_BAR_BUTTON_SIZE.x - TITLE_BAR_BUTTON_GAP;

    spawn_title_bar_button(
        commands,
        title_bar,
        Vec2::new(save_x, 0.0),
        SAVE_BUTTON_COLOR,
        "Save Cache",
        StrategySaveButton,
        |_trigger: Trigger<Pointer<Click>>, mut buffer: ResMut<StrategyBuffer>| {
            let can_save = buffer.cache_path.is_some() && buffer.dirty;
            if !can_save {
                return;
            }
            let Some(path) = buffer.cache_path.clone() else {
                return;
            };
            match std::fs::write(&path, &buffer.source) {
                Ok(()) => {
                    buffer.dirty = false;
                    info!("strategy cache saved: {:?}", path);
                }
                Err(err) => {
                    error!("failed to save strategy cache {:?}: {}", path, err);
                }
            }
        },
    );

    spawn_title_bar_button(
        commands,
        title_bar,
        Vec2::new(run_x, 0.0),
        RUN_BUTTON_COLOR,
        "Run",
        StrategyRunButton,
        |_trigger: Trigger<Pointer<Click>>,
         buffer: Res<StrategyBuffer>,
         last_run: Res<LastRunResult>,
         mut run_events: EventWriter<StrategyRunRequested>| {
            if matches!(last_run.state, RunState::Running) {
                warn!("Run blocked: already running");
                return;
            }
            let can_run = buffer.cache_path.is_some() && !buffer.dirty;
            if !can_run {
                return;
            }
            if let Some(path) = buffer.cache_path.clone() {
                run_events.send(StrategyRunRequested { cache_path: path });
            }
        },
    );
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
        buffer.source = new_text.clone();
        buffer.dirty = true;
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
    // UndoRedoApplied は apply_pending_app_edits_system がテキスト変更時のみ送る。
    // ここで送ると Window spawn/despawn undo でも editor set_text が走りカーソルリセットが起きる。
) {
    *cooldown = (*cooldown - time.delta_secs()).max(0.0);
    if *cooldown > 0.0 {
        return;
    }

    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if !ctrl {
        return;
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);

    let do_undo = keys.just_pressed(KeyCode::KeyZ) && !shift;
    let do_redo = keys.just_pressed(KeyCode::KeyY)
        || (keys.just_pressed(KeyCode::KeyZ) && shift);

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
                buffer.source = text;
                buffer.dirty = true;
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
        buffer.source = snapshot;
        buffer.dirty = true;
        undo_applied.send(UndoRedoApplied);
    }
}

/// Save Cache / Run ボタンの有効/無効を視覚的に反映する system。
///
/// 毎フレーム `StrategyBuffer` を read して、ボタン sprite の alpha を
/// `BUTTON_ENABLED_ALPHA` / `BUTTON_DISABLED_ALPHA` に切り替える。
///
/// クリック自体は observer 側で `can_save` / `can_run` を再判定して
/// 早期 return するので、ここでは見た目だけ揃える役割。
pub fn update_strategy_button_visuals_system(
    buffer: Res<StrategyBuffer>,
    last_run: Res<LastRunResult>,
    mut save_q: Query<&mut Sprite, (With<StrategySaveButton>, Without<StrategyRunButton>)>,
    mut run_q: Query<&mut Sprite, (With<StrategyRunButton>, Without<StrategySaveButton>)>,
) {
    let can_save = buffer.cache_path.is_some() && buffer.dirty;
    let is_running = matches!(last_run.state, RunState::Running);
    let can_run = buffer.cache_path.is_some() && !buffer.dirty && !is_running;

    for mut sprite in &mut save_q {
        sprite.color.set_alpha(if can_save {
            BUTTON_ENABLED_ALPHA
        } else {
            BUTTON_DISABLED_ALPHA
        });
    }
    for mut sprite in &mut run_q {
        sprite.color.set_alpha(if can_run {
            BUTTON_ENABLED_ALPHA
        } else {
            BUTTON_DISABLED_ALPHA
        });
    }
}
