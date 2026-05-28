//! Spike PoC: bevscode `CodeEditor` を world-space floating window に「召喚」する Projected Node 方式の Go/No-Go ゲート。
//!
//! issue #50 Step 0 spike — 詳細は `docs/adr/0006-strategy-editor-projected-ui-overlay.md` と
//! `C:/Users/sasac/.claude/plans/glowing-singing-russell.md` を参照。
//!
//! - bevscode `CodeEditor` は Bevy UI `Node` なので world Transform に直接は追随しない。
//! - 毎フレーム world camera の `OrthographicProjection` で floating window root の world rect を
//!   screen rect に投影し、`Node.left/top/width/height` と `TextFont.font_size = BASE / scale` を上書きする。
//! - drag/pan/zoom 追随は投影で達成、鮮明ズームは font_size スケーリングで bevscode が再ラスタライズ。
//! - 入力（クリック→カーソル）は Node が screen 上の正しい位置にあるので bevscode の picking がそのまま効く想定。
//! - Z オーダー: root sprite を z=200 にピン（projection system が毎フレーム上書き）。Bevy UI は world sprite の後に
//!   描画されるので、editor 中身は常に他 floating window の前面に来る。
//!
//! cosmic_edit 経路（`strategy_editor.rs`）は本 spike では一切触らない。Go 判定後、別 PR で Phase B を着手する。

use crate::ui::floating_window::{FloatingWindowSpec, TITLE_BAR_HEIGHT, spawn_floating_window};
use bevscode::prelude::*;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

/// Spike floating window root sprite に貼る marker。z=200 ピンの識別と、cleanup の追跡に使う。
#[derive(Component)]
pub struct SpikeEditorRoot;

/// Spike `CodeEditor` Node entity に貼る、root への back-link。
#[derive(Component)]
pub struct SpikeEditorNode {
    pub root: Entity,
}

/// メニューから spawn 要求を送るための Message（Bevy 0.18 で `Event`→`Message` 改名）。
#[derive(Message, Debug, Clone, Copy)]
pub struct SpikeEditorSpawnRequested;

/// 投影前のベースフォントサイズ（logical px）。scale=1 のときこの値で描画される。
const BASE_FONT_SIZE: f32 = 14.0;
const BASE_LINE_HEIGHT: f32 = 20.0;
/// Spike エディタ root の世界座標 z 固定値。ADR 0006: 他 floating window（z=10 ベース、+2/click）より常に手前。
const SPIKE_EDITOR_Z: f32 = 200.0;

const PANEL_SIZE: Vec2 = Vec2::new(560.0, 420.0);
const PANEL_POSITION: Vec2 = Vec2::new(280.0, -40.0);
const ACCENT: Color = Color::srgba(0.20, 1.00, 0.45, 0.45); // 緑がかった spike 識別色

const SEED_TEXT: &str = r#"# PoC: bevscode in world-space (issue #50 Step 0)

def strategy(ctx):
    """Toy strategy used only to verify the Projected Node spike."""
    if ctx.position == 0:
        ctx.buy(100)
    else:
        ctx.hold()

# 5 demos to validate Go:
#  1. text renders inside the floating window
#  2. drag the title bar → editor follows
#  3. pan the canvas (right-mouse drag) → editor follows
#  4. zoom in with the wheel → text stays crisp (re-rasterized)
#  5. click on a character → cursor lands there, typing inserts there
"#;

/// Menu / keyboard / 他の入口から `SpikeEditorSpawnRequested` を受けて spike エディタを spawn する handler。
///
/// bevscode の `SetTextRequested` Message を同フレームで write して seed text を流す。
/// `InputFocus` も即座に立てる（example/editor.rs の `setup_editor` と同パターン）。
#[allow(clippy::too_many_arguments)]
pub fn handle_spike_editor_spawn_requests(
    mut events: MessageReader<SpikeEditorSpawnRequested>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut set_text_writer: MessageWriter<SetTextRequested>,
    mut input_focus: ResMut<InputFocus>,
    existing: Query<(), With<SpikeEditorRoot>>,
) {
    for _ in events.read() {
        // dedup: 既に PoC が出ているならスキップ（spike は 1 つで十分）
        if !existing.is_empty() {
            info!("spike editor already spawned; skipping duplicate request");
            continue;
        }

        // 1) 既存の `spawn_floating_window` ヘルパーで world-space シェル（root + title bar + content_area + resize handles）を作る
        let (root, _content_area, _title_bar) = spawn_floating_window(
            &mut commands,
            FloatingWindowSpec {
                title: "PoC: Bevscode".to_string(),
                size: PANEL_SIZE,
                position: PANEL_POSITION,
                accent: ACCENT,
                closeable: true,
                resizable: true,
            },
        );

        // 2) root に spike marker を貼る。z は projection system が毎フレーム SPIKE_EDITOR_Z で
        //    上書きするので、spawn 直後の z=10（spawn_floating_window デフォルト）は最初の 1 フレームのみで OK。
        commands.entity(root).insert(SpikeEditorRoot);

        // 3) CodeEditor Node を peer として spawn（content_area の子にはしない: UI layout は world Transform を無視するため）
        let font: Handle<Font> = asset_server.load("fonts/FiraMono-Regular.ttf");
        let editor = commands
            .spawn((
                CodeEditor,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Px(PANEL_SIZE.x),
                    height: Val::Px(PANEL_SIZE.y - TITLE_BAR_HEIGHT),
                    ..default()
                },
                TextFont::from_font_size(BASE_FONT_SIZE).with_font(font),
                MonoFontFaces::default(),
                bevy::text::LineHeight::Px(BASE_LINE_HEIGHT),
                SpikeEditorNode { root },
                Name::new("SpikeEditorNode"),
            ))
            .id();

        // 4) seed text と focus
        set_text_writer.write(SetTextRequested {
            entity: editor,
            text: SEED_TEXT.to_string(),
        });
        input_focus.set(editor);

        info!("spike editor spawned: root={:?} editor={:?}", root, editor);
    }
}

/// 本 spike の核: 毎フレーム world rect → screen rect 投影で CodeEditor Node を追従させる。
///
/// 投影:
/// ```text
/// scale = ortho.scale  // world units per logical screen pixel
/// content_world_size = (root_size.x, root_size.y - TITLE_BAR_HEIGHT)
/// content_world_topleft = root_center - (root_size.x/2, -root_size.y/2 + TITLE_BAR_HEIGHT)
///   ↑ world Y は上向き、root_center を基準に左上は (center.x - w/2, center.y + h/2 - TITLE_BAR_HEIGHT)
/// screen_topleft.x = (content_world_topleft.x - cam.x) / scale + win_w / 2
/// screen_topleft.y = -(content_world_topleft.y - cam.y) / scale + win_h / 2  ← Y flip
/// node.width/height = content_world_size / scale
/// font_size = BASE_FONT_SIZE / scale
/// ```
///
/// 規約 2 の差分書き込み（spurious `Changed<Node>` で UI layout が無駄に再計算されないよう）。
/// z=200 のピンもここで毎フレーム維持（root の `Press` observer が一時的に bump しても次フレームで戻る）。
///
/// ⚠️ **本 system は `Update` で走り、`Node` のみを更新する**。
/// `ComputedNode` / `UiGlobalTransform` は `PostUpdate` の `bevy::ui::UiSystems::Layout` が
/// 自動計算する。さらに mod.rs の `configure_sets` で
/// `bevy_instanced_text::LayoutProduceSet.after(bevy::ui::UiSystems::Layout)` を強制している。
/// この組み合わせで:
/// - Update: 私たちが `Node.left/top/width/height` を書く（root の current world position から投影）
/// - PostUpdate `UiSystems::Layout`: `ComputedNode` と `UiGlobalTransform` を再計算 + ガター子 Node を再配置
/// - PostUpdate `LayoutProduceSet`（上記の after）: bevscode のテキスト本体が fresh `ComputedNode` を読む
/// - すべて同フレームに揃う（テキスト本体もガターも 1 frame 遅延しない）
///
/// 初版（spike 検証中）は `PostUpdate.before(LayoutProduceSet)` で `ComputedNode` を直接書く
/// 方式だったが、Bevy UI Layout が `LayoutProduceSet` の後に走るため、ガター子 Node の再配置が
/// 1 frame ズレる症状が出た。configure_sets で順序を明示することで両方解決する。
pub fn project_spike_editor_node_system(
    mut roots: Query<(&mut Transform, &Sprite), With<SpikeEditorRoot>>,
    cam_q: Query<(&Transform, &Projection), (With<Camera2d>, Without<SpikeEditorRoot>)>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut editors: Query<
        (&SpikeEditorNode, &mut Node, &mut TextFont, &mut bevy::text::LineHeight),
        Without<SpikeEditorRoot>,
    >,
) {
    let Ok((cam_tf, projection)) = cam_q.single() else {
        return;
    };
    let Projection::Orthographic(ortho) = projection else {
        return;
    };
    let scale = ortho.scale.max(1e-6); // 0 除算ガード
    let Ok(window) = windows.single() else {
        return;
    };
    let win_w = window.width();
    let win_h = window.height();
    let cam = cam_tf.translation.truncate();

    for (marker, mut node, mut font, mut line_height) in editors.iter_mut() {
        let Ok((mut root_tf, root_sprite)) = roots.get_mut(marker.root) else {
            continue;
        };
        let Some(root_size) = root_sprite.custom_size else {
            continue;
        };

        // z=200 を維持（Press observer が `10 + max_z` で上書きしても毎フレームここで戻す）
        if (root_tf.translation.z - SPIKE_EDITOR_Z).abs() > 0.001 {
            root_tf.translation.z = SPIKE_EDITOR_Z;
        }

        let root_center = root_tf.translation.truncate();

        // content area の world 矩形（title bar を除いた window 中身）
        let content_w_world = root_size.x;
        let content_h_world = (root_size.y - TITLE_BAR_HEIGHT).max(10.0);
        // content top-left in world: x は中心 - w/2、y は中心 + (h/2 - TITLE_BAR_HEIGHT)（title bar が上端の世界 Y 高い側）
        let content_tl_world_x = root_center.x - root_size.x / 2.0;
        let content_tl_world_y = root_center.y + root_size.y / 2.0 - TITLE_BAR_HEIGHT;

        // screen 投影（Y は反転）
        let node_left = (content_tl_world_x - cam.x) / scale + win_w / 2.0;
        let node_top = -(content_tl_world_y - cam.y) / scale + win_h / 2.0;
        let node_w = content_w_world / scale;
        let node_h = content_h_world / scale;

        // font / line height は scale 逆比で大きくする → bevscode が再ラスタライズして鮮明
        let new_font_size = (BASE_FONT_SIZE / scale).max(1.0);
        let new_line_height_px = (BASE_LINE_HEIGHT / scale).max(2.0);

        // 差分書き込み（規約 2）
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

/// `SpikeEditorRoot` が despawn されたら対応する CodeEditor Node も一緒に despawn する。
/// 既存 close ボタンの observer は root を despawn するだけなので、ここで Node の後片付けをする。
pub fn cleanup_spike_editor_on_root_despawn(
    mut removed: RemovedComponents<SpikeEditorRoot>,
    editors: Query<(Entity, &SpikeEditorNode)>,
    mut commands: Commands,
) {
    for root_entity in removed.read() {
        for (node_entity, marker) in editors.iter() {
            if marker.root == root_entity {
                if let Ok(mut ec) = commands.get_entity(node_entity) {
                    ec.despawn();
                }
            }
        }
    }
}
