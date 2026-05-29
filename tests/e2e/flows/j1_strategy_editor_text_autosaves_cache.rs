//! J1 strategy_editor_text_autosaves_cache — Strategy Editor でテキストを編集すると fragment が dirty になり、
//! 約 1 秒のデバウンス後に cache `.py` へ自動保存されることを保証する（kind:ui）。
//!
//! `debounced_strategy_autosave_system` が dirty + debounce 経過を検知して `cache_path` へ
//! 書き出す経路を headless で駆動する。`last_change` を過去に設定してデバウンス判定を即通過させ、
//! 実ファイル書き込みを assert する。
//! `BACKCAST_CACHE_DIR` を temp に逃がして実 cache を汚さない（CacheDirGuard パターン）。

use std::ffi::OsString;
use std::time::{Duration, Instant};

use serial_test::serial;

use bevy::prelude::*;

use backcast::ui::components::{
    StrategyBuffer, StrategyEditorId, StrategyFragment, WindowRoot,
};
use backcast::ui::strategy_editor::{
    StrategyAutoSaveState, debounced_strategy_autosave_system,
};

/// `BACKCAST_CACHE_DIR` を test 用に差し替え、Drop で元へ戻す RAII ガード。
struct CacheDirGuard(Option<OsString>);

impl Drop for CacheDirGuard {
    fn drop(&mut self) {
        // SAFETY: テスト終了時に env を元へ戻すだけ。値読み取りと競合しない単一地点で実行する。
        unsafe {
            match &self.0 {
                Some(v) => std::env::set_var("BACKCAST_CACHE_DIR", v),
                None => std::env::remove_var("BACKCAST_CACHE_DIR"),
            }
        }
    }
}

#[test]
#[serial]
fn j1_strategy_editor_text_autosaves_cache() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    std::fs::create_dir_all(&cache_dir).unwrap();

    // strategy ローダの cache 書き込みを temp に逃がす。
    // SAFETY: app 構築前の単一地点で設定し、ガードの Drop で復元する。
    let _cache_guard = {
        let prev = std::env::var_os("BACKCAST_CACHE_DIR");
        unsafe {
            std::env::set_var("BACKCAST_CACHE_DIR", &cache_dir);
        }
        CacheDirGuard(prev)
    };

    let cache_path = dir.path().join("strategy_cache.py");

    let mut app = App::new();

    app.insert_resource(StrategyBuffer {
        original_path: None,
        cache_path: Some(cache_path.clone()),
        last_merged_source: None,
    })
    .insert_resource(StrategyAutoSaveState::default())
    .add_systems(Update, debounced_strategy_autosave_system);

    let region_key = "region_001".to_string();

    // WindowRoot entity に StrategyFragment + StrategyEditorId を置く（root 側）。
    let root = app
        .world_mut()
        .spawn((
            WindowRoot,
            StrategyEditorId {
                region_key: region_key.clone(),
            },
            StrategyFragment {
                source: "x = 1".to_string(),
                dirty: false,
            },
        ))
        .id();

    // ── Phase A: テキスト変更 seam を直接注入 ──
    // 実際の編集経路は bevscode `CodeEditor` 入力 → `sync_bevscode_to_strategy_fragment_system` が
    // StrategyFragment に書き戻す。本テストの主眼はデバウンス → ファイル書き込みなので、
    // bevscode 入力をエミュレートせず fragment を直接 dirty 化するほうが安定している。
    {
        let mut fragment = app.world_mut().get_mut::<StrategyFragment>(root).unwrap();
        fragment.source = "x = 99\ny = 2".to_string();
        fragment.dirty = true;
    }
    {
        let mut auto_save = app.world_mut().resource_mut::<StrategyAutoSaveState>();
        auto_save.dirty = true;
        // デバウンス (1 秒) を即通過させるため last_change を 2 秒前に設定する。
        auto_save.last_change = Some(Instant::now() - Duration::from_secs(2));
    }

    // まだファイルは存在しないはず。
    assert!(!cache_path.exists(), "フラッシュ前はキャッシュファイルが存在しないはず");

    // ── Phase B: debounced_strategy_autosave_system がキャッシュを書き出す ──
    app.update();

    assert!(
        cache_path.exists(),
        "debounce 経過後に cache ファイルが書き出されるはず"
    );

    let written = std::fs::read_to_string(&cache_path).unwrap();
    assert!(
        written.contains("x = 99"),
        "cache ファイルに編集後の fragment 内容が含まれるはず (written={:?})",
        written
    );
    assert!(
        written.contains("y = 2"),
        "cache ファイルに全 fragment が含まれるはず (written={:?})",
        written
    );

    // ── Phase C: dirty フラグがクリアされていること ──
    {
        let auto_save = app.world().resource::<StrategyAutoSaveState>();
        assert!(!auto_save.dirty, "autosave 後 dirty=false になるはず");
        assert!(
            auto_save.last_change.is_none(),
            "autosave 後 last_change=None になるはず"
        );
    }
    {
        let fragment = app.world().get::<StrategyFragment>(root).unwrap();
        assert!(!fragment.dirty, "autosave 後 fragment.dirty=false になるはず");
    }

    // ── Phase D: デバウンス内 (dirty=true だが last_change が直近) はファイルを更新しない ──
    {
        let mut fragment = app.world_mut().get_mut::<StrategyFragment>(root).unwrap();
        fragment.source = "z = 999".to_string();
        fragment.dirty = true;
    }
    {
        let mut auto_save = app.world_mut().resource_mut::<StrategyAutoSaveState>();
        auto_save.dirty = true;
        auto_save.last_change = Some(Instant::now()); // デバウンス未満
    }

    let before = std::fs::read_to_string(&cache_path).unwrap();
    app.update();
    let after = std::fs::read_to_string(&cache_path).unwrap();
    assert_eq!(
        before, after,
        "デバウンス内はキャッシュファイルを書き換えないはず"
    );
}
