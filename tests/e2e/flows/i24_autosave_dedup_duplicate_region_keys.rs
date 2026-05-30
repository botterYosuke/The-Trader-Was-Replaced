//! I24 (kind:unit): 同一 `region_key` を持つ entity が複数存在するとき、
//! `debounced_strategy_autosave_system` が `app_state.py` に重複 `# region` マーカーを
//! 書き込まないことを保証する（issue #62 regression guard）。
//!
//! ## 症状
//!
//! `File → Load... → examples/test_strategy_daily.py`（4-region layout）後、
//! 1 回目の Run は成功するが 2 回目の ▶ で `SyntaxError: invalid syntax` が出る。
//! 原因は `app_state.py` に重複した `# region region_001` マーカーが書き込まれること。
//!
//! ## 根本原因
//!
//! `LayoutRestore` 経路で `apply_pending_layout_system` が in-place 更新に失敗し、
//! 同一 `region_key` を持つ entity が 2 つ存在する場合（または他の原因による重複）、
//! `merge_fragments` が同じキーのマーカーを 2 回生成する。
//!
//! ## fix
//!
//! `merge_fragments` 内で重複 key をスキップ（warn ログ付き）。
//! `debounced_strategy_autosave_system` の items 収集後にも `dedup_by_key` を適用して
//! 二重防衛とする。
//!
//! ## 自動ガード
//!
//! - `i24_merge_fragments_skips_duplicate_region_keys`: 重複 key を含む items を
//!   `merge_fragments` に渡したとき、出力に同一 `# region <key>` が 1 回しか現れないことを assert。
//! - `i24_autosave_dedup_duplicate_entity_region_keys`: 同一 region_key を持つ entity が 2 つ
//!   存在するとき `debounced_strategy_autosave_system` が重複マーカーなしで autosave することを assert。

use std::time::{Duration, Instant};

use bevy::prelude::*;
use serial_test::serial;

use backcast::ui::components::{StrategyBuffer, StrategyEditorId, StrategyFragment, WindowRoot};
use backcast::ui::strategy_editor::{
    StrategyAutoSaveState, debounced_strategy_autosave_system, merge_fragments,
};

/// I24a — `merge_fragments` が重複 region_key を受け取ったとき、
/// 出力に `# region region_001` が 1 回しか現れないことを assert する。
#[test]
fn i24_merge_fragments_skips_duplicate_region_keys() {
    let items = vec![
        ("region_001".to_string(), "class Foo: pass".to_string()),
        // duplicate — 根本原因で entity が 2 つ生成されたときに発生
        ("region_001".to_string(), "class Bar: pass".to_string()),
        ("region_002".to_string(), "class Baz: pass".to_string()),
    ];

    let merged = merge_fragments(&items);

    // `# region region_001` は 1 回だけ現れるはず
    let region_count = merged.matches("# region region_001").count();
    assert_eq!(
        region_count, 1,
        "merge_fragments に重複 region_key を渡したとき \
         '# region region_001' は 1 回だけ出力されるはず (実際: {}) merged={:?}",
        region_count, merged
    );

    // `# endregion region_001` も 1 回だけ
    let endregion_count = merged.matches("# endregion region_001").count();
    assert_eq!(
        endregion_count, 1,
        "'# endregion region_001' は 1 回だけ出力されるはず (実際: {}) merged={:?}",
        endregion_count, merged
    );

    // region_002 はそのまま出力される
    assert!(
        merged.contains("# region region_002"),
        "region_002 は出力に含まれるはず merged={:?}",
        merged
    );
}

/// I24b — `debounced_strategy_autosave_system` が同一 region_key を持つ entity が 2 つあっても
/// 重複マーカーなしで autosave することを assert する。
#[test]
#[serial]
fn i24_autosave_dedup_duplicate_entity_region_keys() {
    let dir = tempfile::tempdir().unwrap();
    let cache_path = dir.path().join("strategy_cache.py");

    let mut app = App::new();
    app.insert_resource(StrategyBuffer {
        original_path: None,
        cache_path: Some(cache_path.clone()),
        last_merged_source: None,
    })
    .insert_resource(StrategyAutoSaveState::default())
    .add_systems(Update, debounced_strategy_autosave_system);

    // 同一 region_key "region_001" を持つ entity を 2 つ spawn（バグ条件の再現）
    app.world_mut().spawn((
        WindowRoot,
        StrategyEditorId {
            region_key: "region_001".to_string(),
        },
        StrategyFragment {
            source: "class BuyAndHoldStrategy: pass".to_string(),
            dirty: false,
        },
    ));
    app.world_mut().spawn((
        WindowRoot,
        StrategyEditorId {
            region_key: "region_001".to_string(), // duplicate!
        },
        StrategyFragment {
            source: "\"\"\"docstring\"\"\"".to_string(),
            dirty: false,
        },
    ));
    app.world_mut().spawn((
        WindowRoot,
        StrategyEditorId {
            region_key: "region_002".to_string(),
        },
        StrategyFragment {
            source: "import backcast".to_string(),
            dirty: false,
        },
    ));

    // debounce を即通過させる
    {
        let mut auto_save = app.world_mut().resource_mut::<StrategyAutoSaveState>();
        auto_save.dirty = true;
        auto_save.last_change = Some(Instant::now() - Duration::from_secs(2));
    }

    app.update();

    let written = std::fs::read_to_string(&cache_path).unwrap_or_default();

    // `# region region_001` は 1 回だけ現れるはず
    let region_count = written.matches("# region region_001").count();
    assert_eq!(
        region_count, 1,
        "autosave 出力に '# region region_001' は 1 回だけ現れるはず (実際: {}) written={:?}",
        region_count, written
    );

    let endregion_count = written.matches("# endregion region_001").count();
    assert_eq!(
        endregion_count, 1,
        "autosave 出力に '# endregion region_001' は 1 回だけ現れるはず (実際: {}) written={:?}",
        endregion_count, written
    );
}
