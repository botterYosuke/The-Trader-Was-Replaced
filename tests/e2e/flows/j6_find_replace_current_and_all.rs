//! J6 find_replace_current_and_all — Find / Replace パネルの Repl / Repl All が
//! 現在マッチまたは全マッチだけを置換した結果を `SetTextRequested` メッセージで
//! bevscode editor に流すことを保証する。Slice 5 (#50): cosmic 経路を撤去し、
//! `replace_execute_system` → `SetTextRequested` の整合性を contract レベルで検証する。
//!
//! `SetTextRequested.text` が新ソース（apply_replacement の純粋関数出力）と一致することを
//! 4 つのシナリオ (Replace current / ReplaceAll / case-insensitive / no-match) で確認する。
//! 実際の bevscode `TextBuffer<RopeBuffer>` 更新と `StrategyFragment` writeback は
//! `sync_bevscode_to_strategy_fragment_system` の責務で、bevscode 側の単体テスト
//! および j1 (autosave) が上流カバレッジを与える。

use bevy::prelude::*;
use bevscode::prelude::SetTextRequested;

use backcast::ui::components::{StrategyEditorId, StrategyFragment, WindowRoot};
use backcast::ui::strategy_editor::StrategyEditorNode;
use backcast::ui::strategy_editor_find::{
    FindActionRequested, FindButtonKind, FindMatchRects, FindReplaceState,
    compute_find_match_spans_system, replace_execute_system,
};

/// 共通セットアップ: WindowRoot + StrategyFragment + bevscode peer (StrategyEditorNode) を
/// spawn し、compute + replace を chain したテスト App を返す。
fn build_app(source: &str) -> (App, Entity) {
    let mut app = App::new();

    app.init_resource::<FindReplaceState>()
        .add_message::<FindActionRequested>()
        .add_message::<SetTextRequested>()
        .add_systems(
            Update,
            (compute_find_match_spans_system, replace_execute_system).chain(),
        );

    let region_key = "region_001".to_string();
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

    let editor_entity = app
        .world_mut()
        .spawn((
            StrategyEditorNode {
                root,
                region_key,
            },
            FindMatchRects::default(),
        ))
        .id();

    (app, editor_entity)
}

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
    }
    // compute_find_match_spans_system を走らせてマッチを埋める (query 変更により current=0 にリセットされる)。
    app.update();
    // 望む current 位置を compute 後に上書き (次フレームの compute は query/target 未変なので early-return)。
    app.world_mut().resource_mut::<FindReplaceState>().current = current;
}

fn drain_set_text(app: &mut App) -> Vec<SetTextRequested> {
    app.world_mut()
        .resource_mut::<Messages<SetTextRequested>>()
        .drain()
        .collect()
}

#[test]
fn j6_find_replace_current_and_all() {
    // ── テスト 1: Replace（現在マッチのみ置換） ──
    {
        let source = "foo bar foo baz foo";
        let (mut app, editor_entity) = build_app(source);
        setup_state(&mut app, editor_entity, "foo", "FOO", true, 1);

        {
            let state = app.world().resource::<FindReplaceState>();
            assert_eq!(state.matches.len(), 3, "マッチ計算後 3 件あるはず");
        }

        // 前フレームの compute だけで来た SetTextRequested(初期 seed) は無いはず。念のため drain。
        let _ = drain_set_text(&mut app);

        app.world_mut()
            .write_message(FindActionRequested(FindButtonKind::Replace));
        app.update();

        let drained = drain_set_text(&mut app);
        assert_eq!(
            drained.len(),
            1,
            "Replace で SetTextRequested が 1 件 flush される"
        );
        assert_eq!(drained[0].entity, editor_entity);
        // current=1 のマッチ (2 番目の "foo") のみ置換。
        assert_eq!(drained[0].text, "foo bar FOO baz foo");
    }

    // ── テスト 2: ReplaceAll（全マッチ置換） ──
    {
        let source = "foo bar foo baz foo";
        let (mut app, editor_entity) = build_app(source);
        setup_state(&mut app, editor_entity, "foo", "X", true, 0);
        let _ = drain_set_text(&mut app);

        app.world_mut()
            .write_message(FindActionRequested(FindButtonKind::ReplaceAll));
        app.update();

        let drained = drain_set_text(&mut app);
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].entity, editor_entity);
        assert_eq!(drained[0].text, "X bar X baz X");
    }

    // ── テスト 3: case-insensitive マッチの置換 ──
    {
        let source = "Foo FOO foo";
        let (mut app, editor_entity) = build_app(source);
        setup_state(&mut app, editor_entity, "foo", "bar", false, 0);
        {
            let state = app.world().resource::<FindReplaceState>();
            assert_eq!(
                state.matches.len(),
                3,
                "case-insensitive で 'Foo'/'FOO'/'foo' が 3 件マッチするはず"
            );
        }
        let _ = drain_set_text(&mut app);

        app.world_mut()
            .write_message(FindActionRequested(FindButtonKind::ReplaceAll));
        app.update();

        let drained = drain_set_text(&mut app);
        assert_eq!(drained.len(), 1);
        // 3 件全てが "bar" に置換される。
        assert_eq!(drained[0].text, "bar bar bar");
        assert!(
            !drained[0].text.to_lowercase().contains("foo"),
            "case-insensitive ReplaceAll 後 'foo' 変種は残らない (got: {:?})",
            drained[0].text
        );
    }

    // ── テスト 4: マッチなし → SetTextRequested が発火しない ──
    {
        let source = "hello world";
        let (mut app, editor_entity) = build_app(source);
        setup_state(&mut app, editor_entity, "nomatch", "X", true, 0);
        let _ = drain_set_text(&mut app);

        app.world_mut()
            .write_message(FindActionRequested(FindButtonKind::Replace));
        app.update();

        let drained = drain_set_text(&mut app);
        assert_eq!(
            drained.len(),
            0,
            "マッチなしのとき SetTextRequested は発火しないはず (got {})",
            drained.len()
        );
    }
}
