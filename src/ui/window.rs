use crate::ui::chart_axes::{PriceGutter, PriceGutterRef, TimeGutter, TimeGutterRef};
use crate::ui::chart_crosshair::CrosshairState;
use crate::ui::chart_ladder_pane::LadderPane;
use crate::ui::chart_viewstate::{
    CHART_CHILD_LOCAL_X_REPLAY, CHART_CHILD_LOCAL_Y, CHART_DRAW_SIZE, CHART_PANEL_SIZE,
    ChartViewState, LADDER_WIDTH, PRICE_GUTTER_WIDTH, TIME_GUTTER_HEIGHT,
};
use crate::ui::components::{
    ChartInstrument, ChartSizeMap, InstrumentRegistry, LayoutExcluded, PanelKind, PriceDisplay,
    WindowRoot,
};
use crate::ui::floating_window::{FloatingWindowSpec, TITLE_BAR_HEIGHT, spawn_floating_window};
use crate::ui::theme::Theme;
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

const PANEL_POSITION: Vec2 = Vec2::new(200.0, 0.0);

pub fn spawn_chart_panel(commands: &mut Commands, instrument_id: &str, initial_size: Vec2) {
    // 枠は共通ヘルパーに任せる
    let (root, content_area, _title_bar) = spawn_floating_window(
        commands,
        FloatingWindowSpec {
            title: format!("CHART — {}", instrument_id),
            size: initial_size,
            position: PANEL_POSITION,
            accent: Theme::default().colors.accent.with_alpha(0.4),
            closeable: true,
            resizable: true,
        },
    );
    commands.entity(root).insert(PanelKind::Chart);
    commands.entity(root).insert(ChartInstrument {
        instrument_id: instrument_id.to_string(),
    });
    commands.entity(root).insert(LayoutExcluded);

    // ─── ここから下は中身（content_area の子として配置） ───

    // Price Display（コンパクト window 用に縮小し draw 領域上端へ）。
    // draw 領域の中心 x に合わせて chart child と同じ量だけ左へ寄せる。
    let price_text = commands
        .spawn((
            Text2d::new("$100.00"),
            TextFont {
                font_size: 22.0,
                ..default()
            },
            TextColor(Theme::default().status.success),
            Transform::from_xyz(CHART_CHILD_LOCAL_X_REPLAY, 72.0, 0.3),
            PriceDisplay,
        ))
        .id();

    // Chart entity。
    // ⚠️ content_area は spawn_floating_window で root-local (0, -title_bar_half) にオフセット済。
    //    window 高さが draw + time gutter + title bar を勘定するようになった (CHART_PANEL_SIZE.y=244)
    //    ので、content 領域 (204px) に chart(180,上) + time gutter(24,下) を縦に積む。
    //    chart は左 PRICE_GUTTER_WIDTH/2・上 TIME_GUTTER_HEIGHT/2 寄せ、右 50px を price gutter、
    //    下 24px を time gutter に空ける (Phase B forward gotcha の確定修正)。
    let chart = commands
        .spawn((
            // ⚠️ Phase C の Pointer<Drag> を成立させる hit-target (Caveat #1)。
            //    Color::NONE (alpha=0) は sprite picking の AlphaThreshold mode で除外され
            //    うるため alpha 0.001 の実質透明 sprite にする。
            Sprite {
                custom_size: Some(CHART_DRAW_SIZE),
                color: Color::NONE.with_alpha(0.001),
                ..default()
            },
            Transform::from_xyz(CHART_CHILD_LOCAL_X_REPLAY, CHART_CHILD_LOCAL_Y, 0.1),
            ChartViewState {
                bounds: CHART_DRAW_SIZE,
                ..default()
            },
            ChartInstrument {
                instrument_id: instrument_id.to_string(),
            },
            // Phase D: crosshair 状態。observer (Pointer<Move>/<Out>) が cursor_world を書き、
            // derive system が hovered_price/time を埋める。
            CrosshairState::default(),
        ))
        .id();

    // Phase B: axis label gutter (chart entity の子、Transform は chart-local)。
    // ラベルは price/time axis system が gutter の子 Text2d として despawn+respawn する。
    let price_gutter = commands
        .spawn((
            PriceGutter,
            Transform::from_xyz(CHART_DRAW_SIZE.x / 2.0 + PRICE_GUTTER_WIDTH / 2.0, 0.0, 0.1),
            Visibility::default(),
        ))
        .id();
    let time_gutter = commands
        .spawn((
            TimeGutter,
            Transform::from_xyz(
                0.0,
                -CHART_DRAW_SIZE.y / 2.0 - TIME_GUTTER_HEIGHT / 2.0,
                0.1,
            ),
            Visibility::default(),
        ))
        .id();
    commands.entity(chart).add_child(price_gutter);
    commands.entity(chart).add_child(time_gutter);
    commands
        .entity(chart)
        .insert((PriceGutterRef(price_gutter), TimeGutterRef(time_gutter)));

    commands.entity(content_area).add_child(price_text);
    commands.entity(content_area).add_child(chart);
    commands.entity(root).insert(ChartLayoutChildren { chart, price_text });
}

/// リサイズ時にチャート描画エリアを追従させるため、root entity に挿入する子エンティティ参照。
#[derive(Component)]
pub struct ChartLayoutChildren {
    pub chart: Entity,
    pub price_text: Entity,
}

/// root Sprite の custom_size が変わったとき chart 描画領域をリフローする。
/// Ladder ペインがある（Live 複合）場合は draw_w から LADDER_WIDTH も引き、
/// ladder pane の x と高さも更新する。
pub fn chart_content_layout_system(
    roots: Query<(Entity, &Sprite, &ChartLayoutChildren), (With<WindowRoot>, Changed<Sprite>)>,
    mut charts: Query<
        (&PriceGutterRef, &TimeGutterRef, &mut ChartViewState, &mut Sprite),
        (Without<WindowRoot>, Without<LadderPane>),
    >,
    mut transforms: Query<&mut Transform, (Without<WindowRoot>, Without<LadderPane>)>,
    mut ladder_panes: Query<(&LadderPane, &mut Sprite, &mut Transform), Without<WindowRoot>>,
) {
    for (root_entity, root_sprite, layout) in &roots {
        let Some(root_size) = root_sprite.custom_size else { continue; };

        // ladder の有無を確認し draw_w を決定する（差分書き込みのため先に iter）。
        let ladder_w = ladder_panes
            .iter()
            .find(|(lp, _, _)| lp.chart_root == root_entity)
            .map(|_| LADDER_WIDTH)
            .unwrap_or(0.0);

        let draw_w = (root_size.x - PRICE_GUTTER_WIDTH - ladder_w).max(10.0);
        let draw_h = (root_size.y - TITLE_BAR_HEIGHT - TIME_GUTTER_HEIGHT).max(10.0);
        let new_size = Vec2::new(draw_w, draw_h);

        let Ok((price_gutter_ref, time_gutter_ref, mut view_state, mut chart_sprite)) =
            charts.get_mut(layout.chart)
        else { continue; };

        if chart_sprite.custom_size != Some(new_size) {
            chart_sprite.custom_size = Some(new_size);
        }
        if view_state.bounds != new_size {
            view_state.bounds = new_size;
        }
        let pg_x = draw_w / 2.0 + PRICE_GUTTER_WIDTH / 2.0;
        if let Ok(mut t) = transforms.get_mut(price_gutter_ref.0) {
            if (t.translation.x - pg_x).abs() > 0.01 { t.translation.x = pg_x; }
        }
        let tg_y = -draw_h / 2.0 - TIME_GUTTER_HEIGHT / 2.0;
        if let Ok(mut t) = transforms.get_mut(time_gutter_ref.0) {
            if (t.translation.y - tg_y).abs() > 0.01 { t.translation.y = tg_y; }
        }

        // ladder ペインの位置と高さを更新する（right-flush）。
        if ladder_w > 0.0 {
            let ladder_x = root_size.x / 2.0 - LADDER_WIDTH / 2.0;
            let ladder_h = root_size.y - TITLE_BAR_HEIGHT;
            for (lp, mut lp_sprite, mut lp_tf) in ladder_panes.iter_mut() {
                if lp.chart_root != root_entity { continue; }
                let new_lp_size = Vec2::new(LADDER_WIDTH, ladder_h);
                if lp_sprite.custom_size != Some(new_lp_size) {
                    lp_sprite.custom_size = Some(new_lp_size);
                }
                if (lp_tf.translation.x - ladder_x).abs() > 0.01 {
                    lp_tf.translation.x = ladder_x;
                }
            }
        }
    }
}

/// `InstrumentRegistry` と Chart `WindowRoot` を同期する。
pub fn instrument_chart_sync_system(
    registry: Res<InstrumentRegistry>,
    chart_q: Query<(Entity, &ChartInstrument), With<WindowRoot>>,
    mut commands: Commands,
    mut map: ResMut<crate::trading::InstrumentTradingDataMap>,
    mut chart_sizes: ResMut<ChartSizeMap>,
) {
    if !registry.is_changed() {
        return;
    }
    let desired: HashSet<&str> = registry.ids.iter().map(|s| s.as_str()).collect();
    let spawned: HashMap<&str, Entity> = chart_q
        .iter()
        .map(|(e, c)| (c.instrument_id.as_str(), e))
        .collect();

    for id in &desired {
        if !spawned.contains_key(id) {
            let size = chart_sizes.map.get(*id).copied().unwrap_or(CHART_PANEL_SIZE);
            spawn_chart_panel(&mut commands, id, size);
        }
    }
    for (id, e) in &spawned {
        if !desired.contains(id) {
            commands.entity(*e).despawn();
            map.map.remove(*id);
            chart_sizes.map.remove(*id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::components::{ChartInstrument, WindowRoot};

    #[test]
    fn spawn_chart_panel_attaches_chart_instrument_to_root() {
        let mut app = App::new();
        app.add_systems(Startup, |mut commands: Commands| {
            spawn_chart_panel(&mut commands, "1301.TSE", CHART_PANEL_SIZE);
        });
        app.update();

        let world = app.world_mut();
        let mut q = world.query_filtered::<&ChartInstrument, With<WindowRoot>>();
        let found: Vec<&ChartInstrument> = q.iter(world).collect();
        assert_eq!(
            found.len(),
            1,
            "expected exactly 1 ChartInstrument on a WindowRoot"
        );
        assert_eq!(found[0].instrument_id, "1301.TSE");
    }

    #[test]
    fn instrument_chart_sync_system_spawns_chart_for_each_registry_id() {
        use crate::ui::components::InstrumentRegistry;
        use crate::ui::window::instrument_chart_sync_system; // ← まだ無い

        let mut app = App::new();
        app.init_resource::<InstrumentRegistry>();
        app.init_resource::<crate::trading::InstrumentTradingDataMap>();
        app.init_resource::<ChartSizeMap>();
        app.add_systems(Update, instrument_chart_sync_system);

        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.replace_all(&["1301.TSE".to_string()]);
        }
        app.update();

        let world = app.world_mut();
        let mut q = world.query_filtered::<&ChartInstrument, With<WindowRoot>>();
        let found: Vec<&ChartInstrument> = q.iter(world).collect();
        assert_eq!(
            found.len(),
            1,
            "registry の 1 銘柄に対し Chart が 1 entity spawn される"
        );
        assert_eq!(found[0].instrument_id, "1301.TSE");
    }

    #[test]
    fn instrument_chart_sync_system_despawns_chart_when_registry_empties() {
        use crate::ui::components::InstrumentRegistry;

        let mut app = App::new();
        app.init_resource::<InstrumentRegistry>();
        app.init_resource::<crate::trading::InstrumentTradingDataMap>();
        app.init_resource::<ChartSizeMap>();
        app.add_systems(Update, instrument_chart_sync_system);

        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.replace_all(&["1301.TSE".to_string()]);
        }
        app.update();
        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.replace_all(&[]);
        }
        app.update();

        let world = app.world_mut();
        let mut q = world.query_filtered::<&ChartInstrument, With<WindowRoot>>();
        assert_eq!(
            q.iter(world).count(),
            0,
            "registry を空にすると Chart が despawn される"
        );
    }

    #[test]
    fn instrument_chart_sync_system_is_idempotent_across_updates() {
        use crate::ui::components::InstrumentRegistry;

        let mut app = App::new();
        app.init_resource::<InstrumentRegistry>();
        app.init_resource::<crate::trading::InstrumentTradingDataMap>();
        app.init_resource::<ChartSizeMap>();
        app.add_systems(Update, instrument_chart_sync_system);

        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.replace_all(&["1301.TSE".to_string(), "7203.TSE".to_string()]);
        }
        app.update();
        app.update();
        app.update();

        let world = app.world_mut();
        let mut q = world.query_filtered::<&ChartInstrument, With<WindowRoot>>();
        let ids: Vec<&str> = q.iter(world).map(|c| c.instrument_id.as_str()).collect();
        assert_eq!(
            ids.len(),
            2,
            "is_changed() で early return、重複 spawn しない"
        );
        assert!(ids.contains(&"1301.TSE"));
        assert!(ids.contains(&"7203.TSE"));
    }

    #[test]
    fn instrument_chart_sync_system_handles_partial_diff() {
        use crate::ui::components::InstrumentRegistry;

        let mut app = App::new();
        app.init_resource::<InstrumentRegistry>();
        app.init_resource::<crate::trading::InstrumentTradingDataMap>();
        app.init_resource::<ChartSizeMap>();
        app.add_systems(Update, instrument_chart_sync_system);

        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.replace_all(&["A.T".to_string(), "B.T".to_string()]);
        }
        app.update();
        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.replace_all(&["A.T".to_string(), "C.T".to_string()]);
        }
        app.update();

        let world = app.world_mut();
        let mut q = world.query_filtered::<&ChartInstrument, With<WindowRoot>>();
        let ids: Vec<&str> = q.iter(world).map(|c| c.instrument_id.as_str()).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"A.T"));
        assert!(ids.contains(&"C.T"));
        assert!(!ids.contains(&"B.T"));
    }

    #[test]
    fn sync_system_removes_map_entry_for_dropped_id() {
        use crate::trading::{InstrumentTradingData, InstrumentTradingDataMap};
        use crate::ui::components::InstrumentRegistry;

        let mut app = App::new();
        app.init_resource::<InstrumentRegistry>();
        app.init_resource::<InstrumentTradingDataMap>();
        app.init_resource::<ChartSizeMap>();
        app.add_systems(Update, instrument_chart_sync_system);

        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.replace_all(&["A.T".to_string(), "B.T".to_string()]);
        }
        {
            let mut map = app.world_mut().resource_mut::<InstrumentTradingDataMap>();
            map.map
                .insert("A.T".to_string(), InstrumentTradingData::default());
            map.map
                .insert("B.T".to_string(), InstrumentTradingData::default());
        }
        app.update();

        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.replace_all(&["A.T".to_string()]);
        }
        app.update();

        let world = app.world_mut();
        let map = world.resource::<InstrumentTradingDataMap>();
        assert!(
            !map.map.contains_key("B.T"),
            "drop された id の map エントリは削除される"
        );
        assert!(
            map.map.contains_key("A.T"),
            "残っている id の map エントリは保持される"
        );

        let desired_len = world.resource::<InstrumentRegistry>().ids.len();
        assert!(
            map.map.len() <= desired_len,
            "map のエントリ数は desired 集合のサイズ以下"
        );
    }
}
