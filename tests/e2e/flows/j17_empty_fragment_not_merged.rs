//! J17 empty_fragment_not_merged — 空の StrategyFragment は merge_fragments に
//! 渡さないこと。
//!
//! test_strategy_daily.py（region が 1 つ）を 4 ウィンドウ（region_001–004）の
//! サイドカーで開くと空 Editor が 3 つ生まれる。▶ を押すと空 region も
//! `app_state.py` に書き込まれ、2 回目の実行でエラーになる（#62）。
//! merge 前に `.retain(|(_, src)| !src.trim().is_empty())` でフィルタする回帰ガード。
//! 詳細は `tests/e2e/FLOWS.md` の J17 を参照。

use crate::support::Harness;
use backcast::trading::{ExecutionMode, ExecutionModeRes};
use backcast::ui::components::{
    InstrumentRegistry, ScenarioMetadata, StrategyBuffer, StrategyEditorId, StrategyFragment,
    WindowRoot,
};

#[test]
fn j17_empty_fragment_not_merged() {
    let mut h = Harness::new();
    let dir = tempfile::TempDir::new().unwrap();

    // Replay mode + 最小有効シナリオ
    h.app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::Replay;
    {
        let mut sc = h.app.world_mut().resource_mut::<ScenarioMetadata>();
        sc.instruments = vec!["7203.TSE".to_string()];
        sc.start = Some("2025-01-06".to_string());
        sc.end = Some("2025-03-31".to_string());
        sc.granularity = Some("Daily".to_string());
        sc.initial_cash = Some(1_000_000);
    }
    h.app
        .world_mut()
        .resource_mut::<InstrumentRegistry>()
        .editable = false;

    // cache_path を事前設定（flush_strategy_cache が Ok(true) を返す条件）
    let cache_py = dir.path().join("cache.py");
    std::fs::write(&cache_py, "").unwrap();
    h.app
        .world_mut()
        .resource_mut::<StrategyBuffer>()
        .cache_path = Some(cache_py);

    // region_001: 実コードあり、region_002/003: 空（#62 の再現ケース）
    h.app.world_mut().spawn((
        WindowRoot,
        StrategyEditorId {
            region_key: "region_001".to_string(),
        },
        StrategyFragment {
            source: "x = 1\n".to_string(),
            dirty: false,
        },
    ));
    h.app.world_mut().spawn((
        WindowRoot,
        StrategyEditorId {
            region_key: "region_002".to_string(),
        },
        StrategyFragment {
            source: String::new(),
            dirty: false,
        },
    ));
    h.app.world_mut().spawn((
        WindowRoot,
        StrategyEditorId {
            region_key: "region_003".to_string(),
        },
        StrategyFragment {
            source: "   ".to_string(), // whitespace only も除外されるべき
            dirty: false,
        },
    ));

    h.set_replay_state(None); // IDLE → ▶ は "new run" パスに入る
    h.click_pause_resume(); // ▶ ボタン
    h.tick();

    let merged = h
        .app
        .world()
        .resource::<StrategyBuffer>()
        .last_merged_source
        .clone()
        .unwrap_or_default();

    let count = merged.matches("# region ").count();
    // RED: 現在は空 region も含まれるため count == 3
    // GREEN (#62 fix 後): 空 region がフィルタされ count == 1
    assert_eq!(
        count,
        1,
        "空 StrategyFragment は merge に含まれないはず (got {count} regions)\n---\n{merged}"
    );
}
