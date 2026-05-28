//! J6 find_replace_current_and_all — Find / Replace パネルの Repl / Repl All が現在マッチまたは
//! 全マッチだけを置換し、case-insensitive / case-sensitive toggle を反映することを保証する（kind:ui）。
//!
//! - `replace_execute_system` が `FindActionRequested(Replace)` で `current` マッチのみ置換。
//! - `replace_execute_system` が `FindActionRequested(ReplaceAll)` で全マッチを置換。
//! - `sync_editor_to_strategy_buffer_system` が `CosmicTextChanged` を受け取り fragment に反映。
//! - case-insensitive マッチ時の置換も検証する。
//! replace_execute_system は `set_text` で buffer を更新して `CosmicTextChanged` を発行する。
//! CosmicEditBuffer を持つ StrategyEditorContent entity が必要。

use bevy::prelude::*;
use bevy_cosmic_edit::cosmic_text::{Attrs, Metrics};
use bevy_cosmic_edit::prelude::FocusedWidget;
use bevy_cosmic_edit::{CosmicEditBuffer, CosmicFontSystem, CosmicTextChanged};
use cosmic_text::FontSystem;

use backcast::ui::components::{StrategyEditorId, StrategyFragment, StrategyBuffer, WindowRoot};
use backcast::ui::editor_history::AppHistory;
use backcast::ui::strategy_editor::{
    StrategyAutoSaveState, StrategyEditorContent, sync_editor_to_strategy_buffer_system,
};
use backcast::ui::strategy_editor_find::{
    FindActionRequested, FindButtonKind, FindReplaceState,
    compute_find_match_spans_system, replace_execute_system,
};
use backcast::ui::strategy_editor_highlight::FindMatchSpans;

/// App に共通のセットアップを行うヘルパ。
///
/// `source` で初期化したフラグメントと editor entity を持つ App を返す。
/// `Find Replace` の完全な経路（compute → replace → sync）を 1 フレームで走らせる。
fn build_app(source: &str) -> (App, Entity, Entity) {
    let mut app = App::new();

    let mut font_system = FontSystem::new();
    let metrics = Metrics::new(14.0, 18.0);

    let buf = CosmicEditBuffer::new(&mut font_system, metrics)
        .with_text(&mut font_system, source, Attrs::new());

    app.insert_resource(CosmicFontSystem(font_system))
        .insert_resource(FocusedWidget(None))
        .init_resource::<FindReplaceState>()
        .insert_resource(StrategyBuffer::default())
        .insert_resource(StrategyAutoSaveState::default())
        .insert_resource(AppHistory::default())
        .insert_resource(ButtonInput::<KeyCode>::default())
        .add_message::<CosmicTextChanged>()
        .add_message::<FindActionRequested>()
        .add_systems(
            Update,
            (
                compute_find_match_spans_system,
                replace_execute_system,
                sync_editor_to_strategy_buffer_system,
            )
                .chain(),
        );

    let region_key = "region_001".to_string();

    // WindowRoot entity（フラグメントを持つ root）。
    let root = app
        .world_mut()
        .spawn((
            WindowRoot,
            StrategyEditorId {
                region_key: region_key.clone(),
            },
            StrategyFragment {
                source: source.to_string(),
                dirty: false,
            },
        ))
        .id();

    // StrategyEditorContent entity（CosmicEditBuffer + FindMatchSpans を持つ child）。
    let editor_entity = app
        .world_mut()
        .spawn((
            StrategyEditorContent,
            StrategyEditorId {
                region_key: region_key.clone(),
            },
            buf,
            FindMatchSpans::default(),
        ))
        .id();

    (app, root, editor_entity)
}

/// FindReplaceState に query/replacement/target/is_open を設定してマッチを compute させる。
fn setup_state(
    app: &mut App,
    editor_entity: Entity,
    query: &str,
    replacement: &str,
    case_sensitive: bool,
    current: usize,
) {
    {
        let mut state = app.world_mut().resource_mut::<FindReplaceState>();
        state.is_open = true;
        state.target_editor = Some(editor_entity);
        state.query = query.to_string();
        state.replacement = replacement.to_string();
        state.case_sensitive = case_sensitive;
        state.current = current;
    }
    // compute_find_match_spans_system を走らせてマッチを埋める。
    app.update();
}

#[test]
fn j6_find_replace_current_and_all() {
    // ── テスト 1: Replace（現在マッチのみ置換） ──
    {
        let source = "foo bar foo baz foo";
        let (mut app, root, editor_entity) = build_app(source);

        // current=1 (2 番目の "foo") を "FOO" に置換。
        setup_state(&mut app, editor_entity, "foo", "FOO", true, 1);

        {
            let state = app.world().resource::<FindReplaceState>();
            assert_eq!(state.matches.len(), 3, "マッチ計算後 3 件あるはず");
        }

        // Replace 実行。
        app.world_mut()
            .write_message(FindActionRequested(FindButtonKind::Replace));
        app.update();

        // fragment が更新されているはず（sync_editor_to_strategy_buffer_system 経由）。
        let fragment = app.world().get::<StrategyFragment>(root).unwrap();
        let result = &fragment.source;
        // "foo bar FOO baz foo" — 2 番目だけ置換、他は残る。
        assert_eq!(
            result.matches("foo").count(),
            2,
            "Replace は current (index=1) のみ置換するので残り 'foo' は 2 つ (got: {:?})",
            result
        );
        assert!(
            result.contains("FOO"),
            "置換後に 'FOO' が含まれるはず (got: {:?})",
            result
        );
    }

    // ── テスト 2: ReplaceAll（全マッチ置換） ──
    {
        let source = "foo bar foo baz foo";
        let (mut app, root, editor_entity) = build_app(source);

        setup_state(&mut app, editor_entity, "foo", "X", true, 0);

        app.world_mut()
            .write_message(FindActionRequested(FindButtonKind::ReplaceAll));
        app.update();

        let fragment = app.world().get::<StrategyFragment>(root).unwrap();
        let result = &fragment.source;
        // 全 "foo" が "X" に置き換わり、"foo" は 0 件残るはず。
        assert!(
            !result.contains("foo"),
            "ReplaceAll 後 'foo' は残らないはず (got: {:?})",
            result
        );
        assert_eq!(
            result.matches('X').count(),
            3,
            "ReplaceAll 後 'X' が 3 箇所になるはず (got: {:?})",
            result
        );
    }

    // ── テスト 3: case-insensitive マッチの置換 ──
    {
        let source = "Foo FOO foo";
        let (mut app, root, editor_entity) = build_app(source);

        // case_sensitive=false で query="foo" → 大小文字問わず全マッチ。
        setup_state(&mut app, editor_entity, "foo", "bar", false, 0);

        {
            let state = app.world().resource::<FindReplaceState>();
            assert_eq!(
                state.matches.len(),
                3,
                "case-insensitive で 'Foo'/'FOO'/'foo' が 3 件マッチするはず"
            );
        }

        app.world_mut()
            .write_message(FindActionRequested(FindButtonKind::ReplaceAll));
        app.update();

        let fragment = app.world().get::<StrategyFragment>(root).unwrap();
        let result = &fragment.source;
        assert!(
            !result.to_lowercase().contains("foo"),
            "case-insensitive ReplaceAll 後 'foo' 変種は残らないはず (got: {:?})",
            result
        );
    }

    // ── テスト 4: マッチなし → CosmicTextChanged が発火しない ──
    {
        let source = "hello world";
        let (mut app, _root, editor_entity) = build_app(source);

        setup_state(&mut app, editor_entity, "nomatch", "X", true, 0);

        // イベントバッファをクリアしてから Replace を実行。
        app.world_mut()
            .resource_mut::<Messages<CosmicTextChanged>>()
            .clear();
        app.world_mut()
            .write_message(FindActionRequested(FindButtonKind::Replace));
        app.update();

        let count = app
            .world_mut()
            .resource_mut::<Messages<CosmicTextChanged>>()
            .drain()
            .count();
        assert_eq!(
            count, 0,
            "マッチなしのとき CosmicTextChanged は発火しないはず (got {count})"
        );
    }
}
