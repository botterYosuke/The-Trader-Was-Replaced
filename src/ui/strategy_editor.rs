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
use crate::ui::strategy_editor_gutter::spawn_line_number_gutter;
use crate::ui::strategy_editor_highlight::{BracketSpans, FindMatchSpans, SyntaxSpans};
use crate::ui::strategy_editor_scrollbar::spawn_editor_scrollbar;
use crate::ui::render_scale::RenderScaleResponsive;
use bevy::prelude::*;
use bevy_cosmic_edit::cosmic_text::{Attrs, AttrsOwned, Edit, Metrics, Shaping};
use bevy_cosmic_edit::{
    CosmicBackgroundColor, CosmicFontSystem, CosmicRenderScale, CosmicTextAlign, CosmicWrap,
    CursorColor, ScrollEnabled,
};
use bevy_cosmic_edit::{CosmicTextChanged, prelude::*};

// ── Bevy native 版 Strategy Editor ─────────────

const PANEL_SIZE: Vec2 = Vec2::new(500.0, 400.0);
const PANEL_POSITION: Vec2 = Vec2::new(-300.0, 50.0);
/// content_area 内でエディタ周辺 (gutter + text + scrollbar) が占める領域。
/// 旧 `EDITOR_SIZE` のリネーム (Caveat #14)。
pub const EDITOR_PANEL_SIZE: Vec2 = Vec2::new(440.0, 320.0);
/// 行番号 gutter の幅。
pub const GUTTER_WIDTH: f32 = 36.0;
/// scrollbar の幅。
pub const SCROLLBAR_WIDTH: f32 = 8.0;
/// テキスト編集領域 (Sprite custom_size) の実サイズ。gutter / scrollbar を除いた分。
pub const EDITOR_TEXT_SIZE: Vec2 = Vec2::new(
    PANEL_SIZE.x - GUTTER_WIDTH - SCROLLBAR_WIDTH,
    EDITOR_PANEL_SIZE.y,
);
const EDITOR_FONT_SIZE: f32 = 14.0;
const EDITOR_LINE_HEIGHT: f32 = 18.0;

/// gutter / editor 共通の cosmic_text Metrics。行高をぴったり一致させて
/// gutter の行番号がエディタの行とズレないようにする (Caveat #4)。
pub fn editor_metrics() -> Metrics {
    Metrics::new(EDITOR_FONT_SIZE, EDITOR_LINE_HEIGHT)
}

/// focused なら CosmicEditor 内部 buffer、unfocused なら CosmicEditBuffer を読む (Caveat #2)。
/// scroll / 行数を読む gutter・scrollbar 系がこの分岐を共有する。
pub fn read_active_buffer<T>(
    editor: Option<&CosmicEditor>,
    buffer: &CosmicEditBuffer,
    f: impl FnOnce(&bevy_cosmic_edit::cosmic_text::Buffer) -> T,
) -> T {
    match editor {
        Some(editor) => editor.with_buffer(f),
        None => f(&buffer.0),
    }
}
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

/// エディタ本体（TextEdit2d 付き sprite）を識別するマーカー。
/// Sub-step 1.8c で `Query<&mut CosmicEditBuffer, With<StrategyEditorContent>>` で取りに行く。
#[derive(Component)]
pub struct StrategyEditorContent;

/// リサイズ時にコンテンツ領域を追従させるため、root entity に挿入する子エンティティ参照。
/// `strategy_editor_content_layout_system` がこれを読んで editor / gutter / scrollbar を更新する。
#[derive(Component)]
pub struct StrategyEditorLayoutChildren {
    pub editor: Entity,
    pub gutter: Entity,
    pub scrollbar_track: Entity,
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
    font_system: &mut CosmicFontSystem,
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

    let (root, content_area, title_bar) = spawn_floating_window(
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
    ));

    let editor = commands
        .spawn((
            TextEdit2d,
            Sprite {
                custom_size: Some(EDITOR_TEXT_SIZE),
                color: Color::WHITE,
                ..default()
            },
            CosmicEditBuffer::new(font_system, editor_metrics()).with_text(
                font_system,
                &seed,
                Attrs::new().color(CosmicColor::rgb(220, 220, 220)),
            ),
            DefaultAttrs(AttrsOwned::new(
                Attrs::new().color(CosmicColor::rgb(220, 220, 220)),
            )),
            CursorColor(Color::WHITE),
            CosmicBackgroundColor(EDITOR_BG),
            Transform::from_xyz(EDITOR_CONTENT_X, 0.0, 0.1),
            StrategyEditorContent,
            // highlight pipeline 用 span コンポーネント (Phase A) + wrap 無効化 (Phase B)。
            // ネストして 1 Bundle 要素に畳む (tuple Bundle の 15 要素上限回避)。
            // CosmicWrap::InfiniteLine: source 行 == layout 行にして gutter 行番号と一致させる。
            (
                SyntaxSpans::default(),
                FindMatchSpans::default(),
                BracketSpans::default(),
                CosmicWrap::InfiniteLine,
            ),
            // editor child にも StrategyEditorId を貼ることで、CosmicTextChanged から
            // region_key を即引きできる (root への親辿りが不要)。
            StrategyEditorId {
                region_key: region_key.clone(),
            },
            RenderScaleResponsive::new(EDITOR_MAX_SUPERSAMPLE),
            CosmicRenderScale(1.0),
            CosmicTextAlign::TopLeft { padding: 8 },
            ScrollEnabled::Disabled,
        ))
        .id();

    commands.entity(content_area).add_child(editor);
    commands.insert_resource(FocusedWidget(Some(editor)));

    // ── Phase B: gutter (左) + scrollbar (右) を editor と横並びに配置 ──
    // content_area 内で [gutter | editor | scrollbar] の幅 EDITOR_PANEL_SIZE.x を中央寄せ。
    let gutter = spawn_line_number_gutter(commands, font_system, region_key.clone(), GUTTER_X);
    commands.entity(content_area).add_child(gutter);

    let scrollbar_track = spawn_editor_scrollbar(commands, editor, SCROLLBAR_X);
    commands.entity(content_area).add_child(scrollbar_track);

    // リサイズ時にコンテンツを追従させるため子エンティティ参照を root に保持する。
    commands.entity(root).insert(StrategyEditorLayoutChildren {
        editor,
        gutter,
        scrollbar_track,
    });

    let _ = title_bar;
    let _ = spec.layout_source;
}

/// content_area 内 (中央 x=0) における [gutter | editor | scrollbar] 群の左端 x。
const GROUP_LEFT_X: f32 = -PANEL_SIZE.x / 2.0;
/// gutter Sprite の中心 x。
pub const GUTTER_X: f32 = GROUP_LEFT_X + GUTTER_WIDTH / 2.0;
/// editor Sprite の中心 x (gutter のぶん右にずらす)。
pub const EDITOR_CONTENT_X: f32 = GROUP_LEFT_X + GUTTER_WIDTH + EDITOR_TEXT_SIZE.x / 2.0;
/// scrollbar Sprite の中心 x。
pub const SCROLLBAR_X: f32 =
    GROUP_LEFT_X + GUTTER_WIDTH + EDITOR_TEXT_SIZE.x + SCROLLBAR_WIDTH / 2.0;

/// Strategy Editor の root サイズが変わったとき（リサイズ）、editor / gutter / scrollbar を
/// 新しいコンテンツ領域サイズに追従させる。差分書き込みで change detection の無駄発火を防ぐ。
pub fn strategy_editor_content_layout_system(
    roots: Query<
        (&Sprite, &StrategyEditorLayoutChildren),
        (With<WindowRoot>, Changed<Sprite>),
    >,
    mut sprites: Query<&mut Sprite, Without<WindowRoot>>,
    mut transforms: Query<&mut Transform, Without<WindowRoot>>,
) {
    for (root_sprite, layout) in &roots {
        let Some(root_size) = root_sprite.custom_size else {
            continue;
        };
        let content_w = root_size.x;
        let content_h = root_size.y - TITLE_BAR_HEIGHT;

        let editor_text_w = (content_w - GUTTER_WIDTH - SCROLLBAR_WIDTH).max(10.0);
        let editor_text_h = content_h.max(10.0);

        let group_left_x = -content_w / 2.0;
        let gutter_x = group_left_x + GUTTER_WIDTH / 2.0;
        let editor_x = group_left_x + GUTTER_WIDTH + editor_text_w / 2.0;
        let scrollbar_x = group_left_x + GUTTER_WIDTH + editor_text_w + SCROLLBAR_WIDTH / 2.0;

        // editor sprite size + position
        if let Ok(mut s) = sprites.get_mut(layout.editor) {
            let target = Vec2::new(editor_text_w, editor_text_h);
            if s.custom_size != Some(target) {
                s.custom_size = Some(target);
            }
        }
        if let Ok(mut t) = transforms.get_mut(layout.editor) {
            if (t.translation.x - editor_x).abs() > 0.01 {
                t.translation.x = editor_x;
            }
        }

        // gutter sprite height + position
        if let Ok(mut s) = sprites.get_mut(layout.gutter) {
            let target = Vec2::new(GUTTER_WIDTH, editor_text_h);
            if s.custom_size != Some(target) {
                s.custom_size = Some(target);
            }
        }
        if let Ok(mut t) = transforms.get_mut(layout.gutter) {
            if (t.translation.x - gutter_x).abs() > 0.01 {
                t.translation.x = gutter_x;
            }
        }

        // scrollbar track height + position
        if let Ok(mut s) = sprites.get_mut(layout.scrollbar_track) {
            let target = Vec2::new(SCROLLBAR_WIDTH, editor_text_h);
            if s.custom_size != Some(target) {
                s.custom_size = Some(target);
            }
        }
        if let Ok(mut t) = transforms.get_mut(layout.scrollbar_track) {
            if (t.translation.x - scrollbar_x).abs() > 0.01 {
                t.translation.x = scrollbar_x;
            }
        }

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
pub fn sync_strategy_buffer_to_editor_system(
    mut open_events: MessageReader<StrategyFileLoadRequested>,
    mut undo_events: MessageReader<UndoRedoApplied>,
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
) {
    open_events.clear();

    if undo_events.read().next().is_none() {
        return;
    }

    for (editor_id, mut edit_buffer, editor_opt) in editor_q.iter_mut() {
        let Some((_, fragment)) = fragments_q
            .iter()
            .find(|(frag_id, _)| frag_id.region_key == editor_id.region_key)
        else {
            continue;
        };
        let source = fragment.source.as_str();
        edit_buffer.set_text(&mut font_system, source, Attrs::new());
        if let Some(mut editor) = editor_opt {
            editor.with_buffer_mut(|b| {
                b.set_text(&mut font_system, source, Attrs::new(), Shaping::Advanced);
                b.set_redraw(true);
            });
        }
    }
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
    mut events: MessageReader<CosmicTextChanged>,
    editor_q: Query<&StrategyEditorId, With<StrategyEditorContent>>,
    mut fragments_q: Query<(&StrategyEditorId, &mut StrategyFragment), With<WindowRoot>>,
    mut history: ResMut<AppHistory>,
    mut auto_save: ResMut<StrategyAutoSaveState>,
) {
    for CosmicTextChanged((entity, new_text)) in events.read() {
        let Ok(editor_id) = editor_q.get(*entity) else {
            continue;
        };
        let region_key = editor_id.region_key.clone();

        let Some((_, mut fragment)) = fragments_q
            .iter_mut()
            .find(|(id, _)| id.region_key == region_key)
        else {
            warn!(
                "CosmicTextChanged for region '{}' but no matching WindowRoot",
                region_key
            );
            continue;
        };

        if let Some((target_key, target_text)) = history.suppress_echo_target.clone() {
            if target_key == region_key && target_text.as_str() == new_text.as_str() {
                history.suppress_echo_target = None;
                fragment.source = new_text.clone();
                continue;
            } else {
                history.suppress_echo_target = None;
            }
        }
        if fragment.source == *new_text {
            continue;
        }
        if !history.is_replaying() {
            history.push_text(
                region_key.clone(),
                fragment.source.clone(),
                new_text.clone(),
            );
        }
        mark_fragment_dirty(&mut fragment, &mut auto_save, new_text.clone());
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
                    commands.entity(entity).despawn_recursive();
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
/// StrategyEditorContent entity が生成されるまで待つ（2 段階遅延）。
pub fn apply_strategy_snapshot_restore_system(
    mut pending_restore: ResMut<PendingStrategySnapshotRestore>,
    mut fragments_q: Query<(&StrategyEditorId, &mut StrategyFragment), With<WindowRoot>>,
    mut history: ResMut<AppHistory>,
    editor_q: Query<Entity, With<StrategyEditorContent>>,
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

        app.world_mut().spawn(StrategyEditorContent);

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
