//! Screen-space draggable floating window (Bevy UI `Node` 流派)。
//!
//! `floating_window.rs` の world-space sprite ウィンドウに対する **screen-space 版**。
//! `bevy_ui_text_input` は screen-space UI 専用なので、Strategy Editor と Startup window は
//! このホストに載せ替える（ADR 0003）。表示専用パネル（buying_power / chart / positions /
//! orders / run_result）は引き続き world-space sprite（`spawn_floating_window`）のまま。
//!
//! 設計:
//! - root は `Node{position_type:Absolute, left/top=Px, width/height=Px, flex_direction:Column}`。
//!   マーカーは `ScreenWindowRoot`（screen-space 識別）＋ `WindowRoot`（m12 可視性システムと
//!   dedup を既存のまま効かせる。world-space の `floating_window_layout_system` は `&Sprite` を
//!   要求するので Node root には一致せず安全にスキップされる）。
//! - タイトルバーのドラッグで root の `left`/`top` を px 単位で更新（screen-space なので
//!   camera scale は掛けない）。
//! - 前面化は `GlobalZIndex` を `Pointer<Pressed>` で `WindowManager.max_z` から採番して上げる。
//!
//! layout 永続化（`layout_persistence`）は world-space `&Sprite`+`&Transform` window に加えて
//! **`ScreenWindowRoot`（Node の left/top/width/height・`GlobalZIndex`）も save/restore する**
//! （#35 follow-up を close）。`build_layout` が screen window を `windows` 配列へ収集し、
//! `apply_layout_system` / `apply_pending_layout_system` が world-space match の空振り後に
//! screen match で `Node` geometry を復元する。Startup は size・可視性を復元しない（size は窓側
//! 定数が正、可視性は `ExecutionMode` 所有 [M9]）。Strategy Editor は size・可視性も layout 権威 [M14]。
//! 保存方向の回帰ガードは [M13]。
//! ⚠️ **残 follow-up**: screen window の close-on-restore（layout に無い editor の despawn）は未対応
//! （world-space のみ）。z-order は GlobalZIndex を round-trip するが `WindowManager.max_z` への
//! 完全統合は未確認。詳細は ADR 0003。

use crate::ui::components::{TitleBar, WindowManager, WindowRoot};
use crate::ui::layout_persistence::AutoSaveState;
use bevy::prelude::*;

/// floating window と同じタイトルバー高さ（見た目を揃える）。
pub const SCREEN_TITLE_BAR_HEIGHT: f32 = 40.0;
const TITLE_PADDING_LEFT: f32 = 16.0;
const CLOSE_BTN_SIZE: f32 = 20.0;

/// screen-space floating window の root マーカー。`layout_persistence` / drag / z-order
/// システムはこれで world-space `WindowRoot` と分岐する。
#[derive(Component)]
pub struct ScreenWindowRoot;

/// screen-space window の close ボタンマーカー（world-space `CloseButton` と区別）。
#[derive(Component)]
pub struct ScreenCloseButton;

/// screen-space window 生成の設定（`FloatingWindowSpec` の screen-space 版）。
#[derive(Clone)]
pub struct ScreenWindowSpec {
    pub title: String,
    /// 幅・高さ（px）。
    pub size: Vec2,
    /// 画面左上原点からの初期位置（left, top）px。
    pub position: Vec2,
    /// タイトルバー下のアクセント色（rim 相当の左ボーダー色）。
    pub accent: Color,
    /// × クローズボタンを出すか（Startup は false）。
    pub closeable: bool,
}

/// `Val::Px` から f32 を取り出す（それ以外は 0.0）。layout/drag で left/top を加算するため。
pub fn px_of(val: Val) -> f32 {
    match val {
        Val::Px(v) => v,
        _ => 0.0,
    }
}

/// `start`（Pointer イベントの `target()`）から `ChildOf` 連鎖を上にたどり、最初の
/// `ScreenWindowRoot` を返す。
///
/// ⚠️ Bevy 0.16 の `Pointer<E>` は bubble するが `trigger.target()` は **最深の被 pick ノード**
/// （タイトル `Text` や `×` glyph など pickable な子）を返す。固定段数の `.parent()` だと
/// 子をクリックした位置で despawn 対象や drag 対象がズレる（title_bar を誤 despawn 等）。
/// root マーカーが見つかるまで上る方式にして depth 非依存にする。
fn find_screen_root(
    start: Entity,
    child_of_q: &Query<&ChildOf>,
    root_q: &Query<(), With<ScreenWindowRoot>>,
) -> Option<Entity> {
    let mut cursor = start;
    for _ in 0..16 {
        if root_q.get(cursor).is_ok() {
            return Some(cursor);
        }
        cursor = child_of_q.get(cursor).ok()?.parent();
    }
    None
}

/// 戻り値: (root, content_area, title_bar)。
/// - root: ウィンドウ全体（Absolute Node）。位置は `left`/`top` を動かす。
/// - content_area: タイトルバー下の `flex_grow:1` 領域。`TextInputNode` 等をここの子にする。
/// - title_bar: タイトルバー Node（右端にボタンを足したい caller 用に公開）。
pub fn spawn_screen_window(
    commands: &mut Commands,
    spec: ScreenWindowSpec,
) -> (Entity, Entity, Entity) {
    // ─── 1. root（Absolute 配置・縦並び） ───
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(spec.position.x),
                top: Val::Px(spec.position.y),
                width: Val::Px(spec.size.x),
                height: Val::Px(spec.size.y),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.07, 0.07, 0.12, 0.96)),
            BorderColor(spec.accent),
            GlobalZIndex(10),
            WindowRoot,
            ScreenWindowRoot,
        ))
        // クリックで前面化（GlobalZIndex を採番して上げる）。target は子ノードのことがあるので
        // ScreenWindowRoot 祖先までたどる。
        .observe(
            |trigger: Trigger<Pointer<Pressed>>,
             child_of_q: Query<&ChildOf>,
             root_marker_q: Query<(), With<ScreenWindowRoot>>,
             mut z_q: Query<&mut GlobalZIndex, With<ScreenWindowRoot>>,
             mut wm: ResMut<WindowManager>| {
                let Some(root) = find_screen_root(trigger.target(), &child_of_q, &root_marker_q)
                else {
                    return;
                };
                wm.max_z += 2.0;
                if let Ok(mut z) = z_q.get_mut(root) {
                    z.0 = 10 + wm.max_z as i32;
                }
            },
        )
        .id();

    // ─── 2. タイトルバー（ドラッグ可能） ───
    let title_bar = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(SCREEN_TITLE_BAR_HEIGHT),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                padding: UiRect::left(Val::Px(TITLE_PADDING_LEFT)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.1, 0.1, 0.2, 1.0)),
            TitleBar,
        ))
        // ドラッグで root の left/top を更新（screen px、camera scale なし）。
        // target はタイトル Text のことがあるので ScreenWindowRoot 祖先までたどる。
        .observe(
            |drag: Trigger<Pointer<Drag>>,
             child_of_q: Query<&ChildOf>,
             root_marker_q: Query<(), With<ScreenWindowRoot>>,
             mut root_q: Query<&mut Node, With<ScreenWindowRoot>>| {
                if drag.event().button != PointerButton::Primary {
                    return;
                }
                let Some(root) = find_screen_root(drag.target(), &child_of_q, &root_marker_q)
                else {
                    return;
                };
                let Ok(mut node) = root_q.get_mut(root) else {
                    return;
                };
                node.left = Val::Px(px_of(node.left) + drag.event().delta.x);
                node.top = Val::Px(px_of(node.top) + drag.event().delta.y);
            },
        )
        // DragEnd で layout autosave を dirty に。
        .observe(
            |_end: Trigger<Pointer<DragEnd>>, mut auto_save: ResMut<AutoSaveState>| {
                auto_save.mark_layout_changed(std::time::Instant::now());
            },
        )
        .id();
    commands.entity(root).add_child(title_bar);

    // ─── 3. タイトル文字 ───
    let title_text = commands
        .spawn((
            Text::new(spec.title.clone()),
            TextFont {
                font_size: 16.0,
                ..default()
            },
            TextColor(Color::WHITE),
        ))
        .id();
    commands.entity(title_bar).add_child(title_text);

    // ─── 4. close ボタン（closeable のときだけ。タイトルバー右端） ───
    if spec.closeable {
        let close_btn = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(8.0),
                    width: Val::Px(CLOSE_BTN_SIZE),
                    height: Val::Px(CLOSE_BTN_SIZE),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.6, 0.15, 0.15, 0.85)),
                Button,
                ScreenCloseButton,
            ))
            .observe(
                |trigger: Trigger<Pointer<Click>>,
                 child_of_q: Query<&ChildOf>,
                 root_marker_q: Query<(), With<ScreenWindowRoot>>,
                 mut commands: Commands,
                 mut auto_save: ResMut<AutoSaveState>| {
                    // target が `×` glyph (Text 子) のこともあるので ScreenWindowRoot 祖先までたどる
                    // （固定段数だと title_bar を誤 despawn する）。
                    let Some(root) = find_screen_root(trigger.target(), &child_of_q, &root_marker_q)
                    else {
                        return;
                    };
                    commands.entity(root).despawn();
                    auto_save.mark_layout_changed(std::time::Instant::now());
                },
            )
            .id();
        commands.entity(title_bar).add_child(close_btn);

        let x = commands
            .spawn((
                Text::new("×"),
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
                TextColor(Color::WHITE),
            ))
            .id();
        commands.entity(close_btn).add_child(x);
    }

    // ─── 5. content area（残り領域。中身をここの子にする） ───
    let content_area = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                ..default()
            },
            Visibility::default(),
        ))
        .id();
    commands.entity(root).add_child(content_area);

    (root, content_area, title_bar)
}
