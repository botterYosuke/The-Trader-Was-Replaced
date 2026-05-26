//! K20 chart_size_sidecar_round_trip — チャートパネルのサイズが SidecarLayout に
//! シリアライズされ、デシリアライズ後に ChartSizeMap へ復元されることを保証する (kind:state)。
//!
//! - `SidecarLayout.chart_sizes` に `HashMap<String, [f32; 2]>` が保存されている。
//! - JSON round-trip で値が失われない。
//! - `apply_layout_system` (または `apply_cache_restore_system`) がロード時に
//!   `ChartSizeMap` を更新する。
//!
//! RED＝回帰ガード・fix は #43 後に green

use backcast::ui::layout_persistence::SidecarLayout;

#[test]
fn k20_chart_size_sidecar_round_trip() {
    // RED: SidecarLayout に chart_sizes フィールドが存在しないためコンパイルエラー or
    //      フィールドが None になりアサートが失敗する。
    let mut sizes = std::collections::HashMap::new();
    sizes.insert("1301.TSE".to_string(), [500.0f32, 320.0f32]);

    let layout = SidecarLayout {
        schema_version: Some(1),
        viewport: None,
        windows: None,
        strategy_path: None,
        selected_symbol: None,
        scenario: None,
        chart_sizes: Some(sizes.clone()),
    };

    let json = serde_json::to_string(&layout).expect("serialization must succeed");
    assert!(
        json.contains("chart_sizes"),
        "JSON に chart_sizes フィールドが含まれるはず。got: {json}"
    );
    assert!(
        json.contains("1301.TSE"),
        "JSON に instrument_id が含まれるはず。got: {json}"
    );

    let restored: SidecarLayout =
        serde_json::from_str(&json).expect("deserialization must succeed");
    let restored_sizes = restored
        .chart_sizes
        .expect("chart_sizes must be Some after round-trip");
    assert_eq!(
        restored_sizes.get("1301.TSE"),
        Some(&[500.0f32, 320.0f32]),
        "round-trip 後も 1301.TSE のサイズ [500, 320] が復元されるはず"
    );
}
