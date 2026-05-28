//! M20 mode_visibility_systems_run_after_status_update — mode 可視性 system 群が
//! **production の registration** (`backcast::ui::add_mode_visibility_systems`) の時点で
//! `ExecutionModeRes` の唯一の writer (`status_update_system`) に対して順序付けられている
//! ことを保証する（kind:wiring）。
//!
//! # 背景
//! M18/M19 はテスト内で自前に `.after(status_update_system)` を張る contract test なので、
//! 「system の logic が正しい順序なら正しく動く」ことは保証するが、**production の mod.rs の
//! 配線そのもの**（`.after` を付け忘れていないか）は観測できない。本フローは production と
//! 同一の registration ヘルパーをそのまま使い、Bevy の schedule build が算出する
//! `conflicting_systems()`（順序付けられておらず、かつアクセスが衝突する system 対）を直接
//! 読む。mode 可視性 system は全て `ExecutionModeRes` を read、`status_update_system` は
//! 同 resource を write するため、`.after` 制約が無いと両者は衝突対として現れる。
//! `.after(status_update_system)` を付けると flattened 依存グラフで接続され衝突対から外れる。
//!
//! 3 段で検証する:
//! - (0) 対象 6 system が全て登録されていること（登録漏れだと (1)(2) が素通りするため）。
//! - (1) 衝突対の不在＝順序付け済みであること（`.after` 欠落だと衝突対に現れる → RED）。
//! - (2) 実行（topsort）順の向き＝`status_update_system` が各可視性 system より前に走ること
//!   （`.before` 等で逆向きに付けると (1) は通ってしまうので向きを別に固定する）。
//! fix 前（`.after` 欠落）は (1) が footer / startup / strategy_editor / order を列挙して RED、
//! fix 後は全て GREEN。
//!
//! system は実行しないので resource のセットアップは不要（`initialize` は param のアクセス
//! 登録だけで、resource の存在は run 時にしか要求されない）。

use bevy::ecs::schedule::{Schedules, SystemKey};
use bevy::prelude::*;

use backcast::backend_sync::status_update_system;
use backcast::ui::add_mode_visibility_systems;

#[test]
fn m20_mode_visibility_systems_run_after_status_update() {
    let mut app = App::new();
    // `.after(status_update_system)` の対象ノードが schedule に存在する必要がある。
    app.add_systems(Update, status_update_system);
    // production と同一の registration（テスト対象）。
    add_mode_visibility_systems(&mut app);

    // Update スケジュールを取り出して build する。`Schedule::initialize` は内部で
    // `Schedules` resource に触るので resource_scope ではなく、Update エントリだけを
    // remove してから world に対して初期化する（Schedules resource 自体は world に残る）。
    let mut schedule = app
        .world_mut()
        .resource_mut::<Schedules>()
        .remove(Update)
        .expect("Update schedule should exist");
    schedule
        .initialize(app.world_mut())
        .expect("Update schedule should build without errors");

    // `build` 後の executable は topsort（実行）順。NodeId→名前 と実行順 index を両方ここから引く
    // （`graph.systems()` の inner は build で executable へ移動して空になるため使えない）。
    let ordered: Vec<(SystemKey, String)> = schedule
        .systems()
        .expect("schedule is initialized")
        .map(|(id, sys)| (id, sys.name().to_string()))
        .collect();
    let name_of = |id: &SystemKey| {
        ordered
            .iter()
            .find(|(nid, _)| nid == id)
            .map(|(_, n)| n.as_str())
            .unwrap_or("")
    };

    const VISIBILITY_SYSTEMS: [&str; 6] = [
        "apply_execution_mode_visibility_system",
        "apply_startup_panel_visibility_system",
        "apply_run_result_visibility_system",
        "apply_strategy_editor_mode_visibility_system",
        "apply_order_button_visibility_system",
        "apply_venue_live_button_visibility_system",
    ];

    // (0) 登録漏れの検出: 対象 6 system が全て schedule に存在すること。これが無いと
    //     `add_mode_visibility_systems` が system を登録し忘れても (1)(2) は素通りする
    //     （衝突対にも現れず、order 検索も Some にならないため）。
    let missing: Vec<&str> = VISIBILITY_SYSTEMS
        .into_iter()
        .filter(|vis| !ordered.iter().any(|(_, n)| n.contains(vis)))
        .collect();
    assert!(
        missing.is_empty(),
        "add_mode_visibility_systems が登録すべき可視性 system が schedule に存在しない: {missing:#?}"
    );

    // (1) `.after` 欠落の検出: status_update_system と衝突したまま（= どちらの向きにも
    //     順序付けられていない）可視性 system が無いこと。
    let mut unordered: Vec<String> = Vec::new();
    for (a, b, _conflicts) in schedule.graph().conflicting_systems().iter() {
        let (na, nb) = (name_of(a), name_of(b));
        if na.contains("status_update_system") {
            unordered.push(nb.to_string());
        } else if nb.contains("status_update_system") {
            unordered.push(na.to_string());
        }
    }
    assert!(
        unordered.is_empty(),
        "mode 可視性 system は production registration で status_update_system に順序付けられて \
         いるべき。順序付けられていない（衝突したままの）system: {unordered:#?}"
    );

    // (2) 向きの検証: status_update_system が各可視性 system より **前** に走ること。
    //     `.before` 等で逆向きに付けても (1) は通る（接続はされる）ので、向きを別に固定する。
    let status_idx = ordered
        .iter()
        .position(|(_, n)| n.contains("status_update_system"))
        .expect("status_update_system should be in the schedule");
    let mut wrong_direction: Vec<String> = Vec::new();
    for vis in VISIBILITY_SYSTEMS {
        if let Some(vis_idx) = ordered.iter().position(|(_, n)| n.contains(vis)) {
            if vis_idx < status_idx {
                wrong_direction.push(vis.to_string());
            }
        }
    }
    assert!(
        wrong_direction.is_empty(),
        "mode 可視性 system は status_update_system より **後** に走るべき（`.after`）。\
         逆向き（status_update より前に実行）の system: {wrong_direction:#?}"
    );
}
