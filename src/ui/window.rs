use crate::ui::chart_viewstate::{CHART_DRAW_SIZE, CHART_PANEL_SIZE, ChartViewState};
use crate::ui::components::{
    ChartInstrument, InstrumentRegistry, LayoutExcluded, PanelKind, PriceDisplay, WindowRoot,
};
use crate::ui::floating_window::{FloatingWindowSpec, spawn_floating_window};
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

const PANEL_POSITION: Vec2 = Vec2::new(200.0, 0.0);
const ACCENT: Color = Color::srgba(0.0, 0.8, 1.0, 0.4);

pub fn spawn_chart_panel(commands: &mut Commands, instrument_id: &str) {
    // 枠は共通ヘルパーに任せる
    let (root, content_area, _title_bar) = spawn_floating_window(
        commands,
        FloatingWindowSpec {
            title: format!("CHART — {}", instrument_id),
            size: CHART_PANEL_SIZE,
            position: PANEL_POSITION,
            accent: ACCENT,
        },
    );
    commands.entity(root).insert(PanelKind::Chart);
    commands.entity(root).insert(ChartInstrument {
        instrument_id: instrument_id.to_string(),
    });
    commands.entity(root).insert(LayoutExcluded);

    // ─── ここから下は中身（content_area の子として配置） ───

    // Price Display（コンパクト window 用に縮小し draw 領域上端へ）
    let price_text = commands
        .spawn((
            Text2d::new("$100.00"),
            TextFont {
                font_size: 22.0,
                ..default()
            },
            TextColor(Color::srgb(0.0, 1.0, 0.5)),
            Transform::from_xyz(0.0, 72.0, 0.3),
            PriceDisplay,
        ))
        .id();

    // Chart entity。
    // ⚠️ content_area は spawn_floating_window で既に root-local (0, -title_bar_half) に
    //    オフセット済 (floating_window.rs:191) なので、draw 領域を title bar 下の領域中央へ
    //    置くには content_area-local y=0 で足りる (Caveat #33: プラン skeleton の
    //    CHART_Y_OFFSET=-TITLE_BAR_HEIGHT/2 は「chart を root 直下に置く」前提の値で、
    //    content_area 子なら 0 が同じ world 位置になる)。
    const CHART_Y_OFFSET: f32 = 0.0;
    let chart = commands
        .spawn((
            // ⚠️ Phase C の Pointer<Drag> を成立させる hit-target (Caveat #1)。
            //    Color::NONE (alpha=0) は sprite picking の AlphaThreshold mode で除外され
            //    うるため alpha 0.001 の実質透明 sprite にする。
            Sprite {
                custom_size: Some(CHART_DRAW_SIZE),
                color: Color::srgba(0.0, 0.0, 0.0, 0.001),
                ..default()
            },
            Transform::from_xyz(0.0, CHART_Y_OFFSET, 0.1),
            ChartViewState {
                bounds: CHART_DRAW_SIZE,
                ..default()
            },
            ChartInstrument {
                instrument_id: instrument_id.to_string(),
            },
        ))
        .id();

    commands.entity(content_area).add_child(price_text);
    commands.entity(content_area).add_child(chart);
}

/// `InstrumentRegistry` と Chart `WindowRoot` を同期する。
pub fn instrument_chart_sync_system(
    registry: Res<InstrumentRegistry>,
    chart_q: Query<(Entity, &ChartInstrument), With<WindowRoot>>,
    mut commands: Commands,
    mut map: ResMut<crate::trading::InstrumentTradingDataMap>,
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
            spawn_chart_panel(&mut commands, id);
        }
    }
    for (id, e) in &spawned {
        if !desired.contains(id) {
            commands.entity(*e).despawn_recursive();
            map.map.remove(*id);
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
            spawn_chart_panel(&mut commands, "1301.TSE");
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
        app.add_systems(Update, instrument_chart_sync_system);

        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.replace_all(&["A.T".to_string(), "B.T".to_string()]);
        }
        {
            let mut map = app
                .world_mut()
                .resource_mut::<InstrumentTradingDataMap>();
            map.map.insert("A.T".to_string(), InstrumentTradingData::default());
            map.map.insert("B.T".to_string(), InstrumentTradingData::default());
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
