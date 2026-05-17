use bevy::prelude::*;
use bevy_cosmic_edit::cosmic_text::{Attrs, Metrics};
use bevy_cosmic_edit::prelude::*;
use bevy_cosmic_edit::{
    CosmicBackgroundColor, CosmicRenderScale, CosmicTextAlign, CursorColor, ScrollEnabled,
};
use chrono::NaiveDate;

use crate::replay::startup_progress::ReplayStartupProgress;
use bevy_cosmic_edit::CosmicTextChanged;

/// `CosmicEditBuffer` から現在の text を取り出す。crate 内部の `BufferExtras::get_text`
/// は private module なので、ここでは `Buffer.lines` から直接 join する fallback を使う。
fn buffer_text(buffer: &CosmicEditBuffer) -> String {
    let mut out = String::new();
    let n = buffer.lines.len();
    for (i, line) in buffer.lines.iter().enumerate() {
        out.push_str(line.text());
        if i + 1 < n {
            out.push('\n');
        }
    }
    out
}

use crate::ui::components::{
    GranularityChoice, ScenarioMetadata, ScenarioStartupCashFieldHost,
    ScenarioStartupEndFieldHost, ScenarioStartupErrorLabel, ScenarioStartupField,
    ScenarioStartupFieldEditor, ScenarioStartupGranularityDailyButton,
    ScenarioStartupGranularityMinuteButton, ScenarioStartupPanelRoot, ScenarioStartupParams,
    ScenarioStartupStartFieldHost, ScenarioWritebackPaths, SidebarRoot,
};

impl GranularityChoice {
    /// canonical 表記 "Daily" / "Minute" に変換する。
    /// ScenarioMetadata.granularity / cache sidecar JSON / StrategyRunConfig.granularity
    /// の三者で同一表記を使うための単一ポイント。
    pub fn as_canonical_str(self) -> &'static str {
        match self {
            GranularityChoice::Daily => "Daily",
            GranularityChoice::Minute => "Minute",
        }
    }

    /// canonical 完全一致で parse する。"daily" や " Daily " などは弾く (None を返す)。
    /// 計画書 §Phase 7.6b / Granularity canonical form の "完全一致のみ" 要件。
    pub fn parse_canonical(s: &str) -> Option<Self> {
        match s {
            "Daily" => Some(GranularityChoice::Daily),
            "Minute" => Some(GranularityChoice::Minute),
            _ => None,
        }
    }
}

/// UI 層（cosmic-edit / button）と validation / state mutation を切り離すための
/// commit イベント。I3 で input widget が出来たら同じ Event を emit する。
/// テストでも `app.world_mut().send_event(...)` で純粋 ECS テストが書ける。
#[derive(Event, Debug, Clone)]
pub enum ScenarioStartupParamCommit {
    Start(String),
    End(String),
    Granularity(GranularityChoice),
    InitialCash(String),
}

/// Sidebar の child として Scenario Startup パネルの Node ツリーを生やす。
/// I3a 段階では cosmic-edit を埋め込まず、3 個の field host (Start/End/Cash) は
/// 空の placeholder Node。Granularity だけは segmented button 2 個を入れる。
/// エラーラベルは空文字で先に entity を確保する。
pub fn spawn_scenario_startup_panel(
    mut commands: Commands,
    sidebar_q: Query<Entity, With<SidebarRoot>>,
) {
    let Ok(sidebar) = sidebar_q.get_single() else {
        return;
    };

    let panel_bg = Color::srgba(0.07, 0.07, 0.11, 1.0);
    let border = Color::srgba(0.18, 0.18, 0.28, 1.0);
    let label_color = Color::srgb(0.78, 0.82, 0.92);
    let header_color = Color::srgb(0.50, 0.70, 1.00);
    let btn_bg = Color::srgba(0.10, 0.10, 0.16, 1.0);
    let btn_text = Color::srgb(0.78, 0.82, 0.92);
    let error_color = Color::srgb(0.95, 0.45, 0.45);

    commands.entity(sidebar).with_children(|parent| {
        parent
            .spawn((
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(4.0)),
                    border: UiRect::top(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(panel_bg),
                BorderColor(border),
                ScenarioStartupPanelRoot,
            ))
            .with_children(|panel| {
                // section header
                panel.spawn((
                    Text::new("Startup"),
                    TextFont {
                        font_size: 10.0,
                        ..default()
                    },
                    TextColor(header_color),
                    Node {
                        padding: UiRect::axes(Val::Px(2.0), Val::Px(2.0)),
                        ..default()
                    },
                ));

                // Start 行
                panel
                    .spawn(Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        padding: UiRect::axes(Val::Px(2.0), Val::Px(2.0)),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Text::new("Start"),
                            TextFont {
                                font_size: 11.0,
                                ..default()
                            },
                            TextColor(label_color),
                            Node {
                                width: Val::Px(70.0),
                                ..default()
                            },
                        ));
                        row.spawn((
                            Node {
                                flex_grow: 1.0,
                                height: Val::Px(18.0),
                                ..default()
                            },
                            ScenarioStartupStartFieldHost,
                        ));
                    });
                panel.spawn((
                    Text::new(""),
                    TextFont {
                        font_size: 10.0,
                        ..default()
                    },
                    TextColor(error_color),
                    Node {
                        padding: UiRect::left(Val::Px(70.0)),
                        ..default()
                    },
                    ScenarioStartupErrorLabel {
                        field: ScenarioStartupField::Start,
                    },
                ));

                // End 行
                panel
                    .spawn(Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        padding: UiRect::axes(Val::Px(2.0), Val::Px(2.0)),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Text::new("End"),
                            TextFont {
                                font_size: 11.0,
                                ..default()
                            },
                            TextColor(label_color),
                            Node {
                                width: Val::Px(70.0),
                                ..default()
                            },
                        ));
                        row.spawn((
                            Node {
                                flex_grow: 1.0,
                                height: Val::Px(18.0),
                                ..default()
                            },
                            ScenarioStartupEndFieldHost,
                        ));
                    });
                panel.spawn((
                    Text::new(""),
                    TextFont {
                        font_size: 10.0,
                        ..default()
                    },
                    TextColor(error_color),
                    Node {
                        padding: UiRect::left(Val::Px(70.0)),
                        ..default()
                    },
                    ScenarioStartupErrorLabel {
                        field: ScenarioStartupField::End,
                    },
                ));

                // Granularity 行 (segmented button)
                panel
                    .spawn(Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        padding: UiRect::axes(Val::Px(2.0), Val::Px(2.0)),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Text::new("Granularity"),
                            TextFont {
                                font_size: 11.0,
                                ..default()
                            },
                            TextColor(label_color),
                            Node {
                                width: Val::Px(70.0),
                                ..default()
                            },
                        ));
                        row.spawn((
                            Button,
                            Node {
                                padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                                margin: UiRect::right(Val::Px(2.0)),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            BackgroundColor(btn_bg),
                            ScenarioStartupGranularityDailyButton,
                        ))
                        .with_children(|btn| {
                            btn.spawn((
                                Text::new("Daily"),
                                TextFont {
                                    font_size: 10.0,
                                    ..default()
                                },
                                TextColor(btn_text),
                            ));
                        });
                        row.spawn((
                            Button,
                            Node {
                                padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            BackgroundColor(btn_bg),
                            ScenarioStartupGranularityMinuteButton,
                        ))
                        .with_children(|btn| {
                            btn.spawn((
                                Text::new("Minute"),
                                TextFont {
                                    font_size: 10.0,
                                    ..default()
                                },
                                TextColor(btn_text),
                            ));
                        });
                    });
                panel.spawn((
                    Text::new(""),
                    TextFont {
                        font_size: 10.0,
                        ..default()
                    },
                    TextColor(error_color),
                    Node {
                        padding: UiRect::left(Val::Px(70.0)),
                        ..default()
                    },
                    ScenarioStartupErrorLabel {
                        field: ScenarioStartupField::Granularity,
                    },
                ));

                // Initial cash 行
                panel
                    .spawn(Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        padding: UiRect::axes(Val::Px(2.0), Val::Px(2.0)),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Text::new("Initial cash"),
                            TextFont {
                                font_size: 11.0,
                                ..default()
                            },
                            TextColor(label_color),
                            Node {
                                width: Val::Px(70.0),
                                ..default()
                            },
                        ));
                        row.spawn((
                            Node {
                                flex_grow: 1.0,
                                height: Val::Px(18.0),
                                ..default()
                            },
                            ScenarioStartupCashFieldHost,
                        ));
                    });
                panel.spawn((
                    Text::new(""),
                    TextFont {
                        font_size: 10.0,
                        ..default()
                    },
                    TextColor(error_color),
                    Node {
                        padding: UiRect::left(Val::Px(70.0)),
                        ..default()
                    },
                    ScenarioStartupErrorLabel {
                        field: ScenarioStartupField::InitialCash,
                    },
                ));

                // Cross-field error
                panel.spawn((
                    Text::new(""),
                    TextFont {
                        font_size: 10.0,
                        ..default()
                    },
                    TextColor(error_color),
                    Node {
                        padding: UiRect::all(Val::Px(2.0)),
                        ..default()
                    },
                    ScenarioStartupErrorLabel {
                        field: ScenarioStartupField::CrossField,
                    },
                ));
            });
    });
}

/// Spawn 3 cosmic-edit `TextEdit` fields (Start / End / InitialCash) as children of
/// their respective field host nodes (created by `spawn_scenario_startup_panel`).
///
/// - `TextEdit` を使う (UI ツリー用)。Sprite 版の `TextEdit2d` ではない。
/// - `Without<ScenarioStartupFieldEditor>` filter で host に二重 spawn しない。
/// - `CosmicRenderScale(1.0)` で supersample を上げず DPI トラップを避ける。
/// - フォーカスは `change_active_editor_ui` が click で切り替えるので、`FocusedWidget` は触らない。
pub fn spawn_scenario_startup_input_fields(
    mut commands: Commands,
    mut font_system: ResMut<bevy_cosmic_edit::prelude::CosmicFontSystem>,
    start_host_q: Query<Entity, (With<ScenarioStartupStartFieldHost>, Without<ScenarioStartupFieldEditor>)>,
    end_host_q: Query<Entity, (With<ScenarioStartupEndFieldHost>, Without<ScenarioStartupFieldEditor>)>,
    cash_host_q: Query<Entity, (With<ScenarioStartupCashFieldHost>, Without<ScenarioStartupFieldEditor>)>,
) {
    fn spawn_field(
        commands: &mut Commands,
        font_system: &mut CosmicFontSystem,
        host: Entity,
        field: ScenarioStartupField,
    ) {
        let entity = commands
            .spawn((
                TextEdit,
                CosmicEditBuffer::new(font_system, Metrics::new(12.0, 14.0)).with_text(
                    font_system,
                    "",
                    Attrs::new().color(CosmicColor::rgb(220, 220, 220)),
                ),
                CursorColor(Color::WHITE),
                CosmicBackgroundColor(Color::srgba(0.02, 0.02, 0.04, 1.0)),
                CosmicRenderScale(1.0),
                CosmicTextAlign::TopLeft { padding: 4 },
                ScrollEnabled::Disabled,
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                ScenarioStartupFieldEditor { field },
            ))
            .id();
        commands.entity(host).add_child(entity);
    }

    if let Ok(host) = start_host_q.get_single() {
        spawn_field(&mut commands, &mut font_system, host, ScenarioStartupField::Start);
    }
    if let Ok(host) = end_host_q.get_single() {
        spawn_field(&mut commands, &mut font_system, host, ScenarioStartupField::End);
    }
    if let Ok(host) = cash_host_q.get_single() {
        spawn_field(&mut commands, &mut font_system, host, ScenarioStartupField::InitialCash);
    }
}

/// ScenarioMetadata → ScenarioStartupParams への片方向 sync。
///
/// - `progress.visible == true` の間は触らない（§Disable enforcement）
/// - `params.dirty == true` の間も触らない（UI 入力中保護）
/// - validation エラー以外の errors は触らない
///   （commit 起因の error を sync で消さない、再 commit で消える）
pub fn sync_startup_params_from_scenario_system(
    metadata: Res<ScenarioMetadata>,
    progress: Res<ReplayStartupProgress>,
    mut params: ResMut<ScenarioStartupParams>,
) {
    if progress.visible {
        return;
    }
    if params.dirty {
        return;
    }

    params.start = metadata.start.clone().unwrap_or_default();
    params.end = metadata.end.clone().unwrap_or_default();

    match metadata.granularity.as_deref() {
        Some("Daily") => {
            params.granularity = GranularityChoice::Daily;
            params.errors.granularity = None;
        }
        Some("Minute") => {
            params.granularity = GranularityChoice::Minute;
            params.errors.granularity = None;
        }
        Some(other) => {
            params.granularity = GranularityChoice::default();
            params.errors.granularity = Some(format!(
                "unknown granularity '{}'; please select Daily or Minute to enable Run",
                other
            ));
        }
        None => {
            params.granularity = GranularityChoice::default();
            params.errors.granularity =
                Some("Please select a granularity to enable Run".into());
        }
    }

    params.initial_cash = match metadata.initial_cash {
        Some(n) => n.to_string(),
        None => String::new(),
    };
}

/// UI commit イベントを受けて validate → ScenarioMetadata 更新 →
/// errors / dirty / writeback_pending を立てる system。
///
/// `progress.visible == true` のときは events を drain しつつ何もしない。
pub fn commit_startup_params_to_scenario_system(
    mut events: EventReader<ScenarioStartupParamCommit>,
    mut params: ResMut<ScenarioStartupParams>,
    mut metadata: ResMut<ScenarioMetadata>,
    progress: Res<ReplayStartupProgress>,
) {
    if progress.visible {
        // disabled: drain events without mutating state
        for _ in events.read() {}
        return;
    }

    let mut any_field_cleared = false;

    for ev in events.read() {
        match ev {
            ScenarioStartupParamCommit::Start(s) => {
                if s.is_empty() {
                    params.errors.start = Some("start must not be empty".into());
                } else if NaiveDate::parse_from_str(s, "%Y-%m-%d").is_err() {
                    params.errors.start =
                        Some(format!("invalid date '{}'; use YYYY-MM-DD", s));
                } else {
                    params.start = s.clone();
                    params.errors.start = None;
                    metadata.start = Some(s.clone());
                    any_field_cleared = true;
                }
            }
            ScenarioStartupParamCommit::End(s) => {
                if s.is_empty() {
                    params.errors.end = Some("end must not be empty".into());
                } else if NaiveDate::parse_from_str(s, "%Y-%m-%d").is_err() {
                    params.errors.end =
                        Some(format!("invalid date '{}'; use YYYY-MM-DD", s));
                } else {
                    params.end = s.clone();
                    params.errors.end = None;
                    metadata.end = Some(s.clone());
                    any_field_cleared = true;
                }
            }
            ScenarioStartupParamCommit::Granularity(g) => {
                params.granularity = *g;
                params.errors.granularity = None;
                metadata.granularity = Some(g.as_canonical_str().to_string());
                any_field_cleared = true;
            }
            ScenarioStartupParamCommit::InitialCash(s) => match s.parse::<i64>() {
                Err(_) => {
                    params.errors.initial_cash = Some("invalid integer".into());
                }
                Ok(n) if n <= 0 => {
                    params.errors.initial_cash =
                        Some("initial cash must be positive".into());
                }
                Ok(n) => {
                    params.initial_cash = s.clone();
                    params.errors.initial_cash = None;
                    metadata.initial_cash = Some(n);
                    any_field_cleared = true;
                }
            },
        }
    }

    // cross-field check: start <= end when both parse OK
    let start_parsed = NaiveDate::parse_from_str(&params.start, "%Y-%m-%d").ok();
    let end_parsed = NaiveDate::parse_from_str(&params.end, "%Y-%m-%d").ok();
    if let (Some(sd), Some(ed)) = (start_parsed, end_parsed) {
        if sd > ed {
            params.errors.cross_field = Some("start must be on or before end".into());
        } else {
            params.errors.cross_field = None;
        }
    }

    if any_field_cleared {
        params.dirty = false;
        params.writeback_pending = true;
    }
}

/// `ScenarioStartupParams.writeback_pending == true` のとき、cache sidecar JSON の
/// `scenario.{start,end,granularity,initial_cash}` だけを書き戻す。
///
/// - `progress.visible == true` の間は何もしない。
/// - `cache_sidecar` 未設定なら no-op (error log なし)。
/// - 既存ファイルを read_json_with_bom_strip で読み直し、4 field 以外
///   (instruments / schema_version / 他 unknown key / layout 等) は触らない。
/// - tmp file に書いて `std::fs::rename` で atomic に置換する。
/// - 成功時のみ `writeback_pending = false`。失敗時は `writeback_pending` を据え置く。
pub fn write_startup_params_to_cache_sidecar_system(
    mut params: ResMut<ScenarioStartupParams>,
    paths: Res<ScenarioWritebackPaths>,
    progress: Res<ReplayStartupProgress>,
) {
    if progress.visible {
        return;
    }
    if !params.writeback_pending {
        return;
    }
    let Some(path) = paths.cache_sidecar.as_deref() else {
        return;
    };

    match rewrite_scenario_startup_params_atomic(
        path,
        &params.start,
        &params.end,
        params.granularity.as_canonical_str(),
        &params.initial_cash,
    ) {
        Ok(()) => {
            params.writeback_pending = false;
        }
        Err(e) => {
            warn!(
                "startup params writeback failed: {:?}: {}",
                path, e
            );
        }
    }
}

/// `ScenarioStartupParams.{start,end,initial_cash}` → 各 cosmic-edit field の text を
/// 一方向 sync (params 側を真として描画を合わせる)。
///
/// - `params.is_changed()` でない tick は早期 return: 毎フレーム書き換えると
///   cosmic-edit 側のカーソル位置がリセットされる典型バグになる。
/// - 現在の buffer text が expected と一致していたら touch しない。
/// - Granularity / CrossField は対象外。
pub fn sync_startup_param_editors_text_system(
    params: Res<ScenarioStartupParams>,
    mut font_system: ResMut<CosmicFontSystem>,
    mut editors_q: Query<(&ScenarioStartupFieldEditor, &mut CosmicEditBuffer)>,
) {
    if !params.is_changed() {
        return;
    }

    for (editor, mut buffer) in editors_q.iter_mut() {
        let expected: &str = match editor.field {
            ScenarioStartupField::Start => &params.start,
            ScenarioStartupField::End => &params.end,
            ScenarioStartupField::InitialCash => &params.initial_cash,
            ScenarioStartupField::Granularity | ScenarioStartupField::CrossField => continue,
        };
        let current = buffer_text(&buffer);
        if current == expected {
            continue;
        }
        buffer.set_text(
            &mut font_system,
            expected,
            Attrs::new().color(CosmicColor::rgb(220, 220, 220)),
        );
    }
}

/// `CosmicTextChanged` event を field marker で引いて
/// `ScenarioStartupParamCommit` を発火 + `params.dirty = true`。
///
/// - `progress.visible` の間は events を drain して return。
/// - editors_q に居ない entity (他の cosmic-edit field) は無視。
pub fn scenario_startup_param_input_system(
    mut events: EventReader<CosmicTextChanged>,
    editors_q: Query<&ScenarioStartupFieldEditor>,
    mut commit_w: EventWriter<ScenarioStartupParamCommit>,
    mut params: ResMut<ScenarioStartupParams>,
    progress: Res<ReplayStartupProgress>,
) {
    if progress.visible {
        for _ in events.read() {}
        return;
    }

    for ev in events.read() {
        let (entity, new_text) = &ev.0;
        let Ok(editor) = editors_q.get(*entity) else {
            continue;
        };
        match editor.field {
            ScenarioStartupField::Start => {
                commit_w.send(ScenarioStartupParamCommit::Start(new_text.clone()));
                params.dirty = true;
            }
            ScenarioStartupField::End => {
                commit_w.send(ScenarioStartupParamCommit::End(new_text.clone()));
                params.dirty = true;
            }
            ScenarioStartupField::InitialCash => {
                commit_w.send(ScenarioStartupParamCommit::InitialCash(new_text.clone()));
                params.dirty = true;
            }
            ScenarioStartupField::Granularity | ScenarioStartupField::CrossField => {}
        }
    }
}

/// Daily / Minute segmented button の click → `Commit::Granularity(..)` 発火 + dirty=true。
///
/// - `progress.visible` の間は何もしない。
/// - 同 tick で両方 Pressed の場合は両方発火しても良い (実用上起きない)。
pub fn scenario_startup_granularity_button_system(
    daily_q: Query<
        &Interaction,
        (
            Changed<Interaction>,
            With<ScenarioStartupGranularityDailyButton>,
        ),
    >,
    minute_q: Query<
        &Interaction,
        (
            Changed<Interaction>,
            With<ScenarioStartupGranularityMinuteButton>,
        ),
    >,
    mut commit_w: EventWriter<ScenarioStartupParamCommit>,
    mut params: ResMut<ScenarioStartupParams>,
    progress: Res<ReplayStartupProgress>,
) {
    if progress.visible {
        return;
    }

    for interaction in daily_q.iter() {
        if matches!(interaction, Interaction::Pressed) {
            commit_w.send(ScenarioStartupParamCommit::Granularity(
                GranularityChoice::Daily,
            ));
            params.dirty = true;
        }
    }
    for interaction in minute_q.iter() {
        if matches!(interaction, Interaction::Pressed) {
            commit_w.send(ScenarioStartupParamCommit::Granularity(
                GranularityChoice::Minute,
            ));
            params.dirty = true;
        }
    }
}

/// granularity ボタンの active 表示 + error label 文言 + disabled 視覚化を毎フレーム描画する。
///
/// - active=濃い青, inactive=濃いグレー。`progress.visible` のときは alpha 0.5 で disabled 風に。
/// - error label の text は `params.errors.{field}` の Some(msg)/None に同期。
pub fn update_scenario_startup_param_ui_system(
    params: Res<ScenarioStartupParams>,
    progress: Res<ReplayStartupProgress>,
    mut daily_bg_q: Query<
        &mut BackgroundColor,
        (
            With<ScenarioStartupGranularityDailyButton>,
            Without<ScenarioStartupGranularityMinuteButton>,
        ),
    >,
    mut minute_bg_q: Query<
        &mut BackgroundColor,
        (
            With<ScenarioStartupGranularityMinuteButton>,
            Without<ScenarioStartupGranularityDailyButton>,
        ),
    >,
    mut label_q: Query<(&ScenarioStartupErrorLabel, &mut Text)>,
) {
    let alpha = if progress.visible { 0.5 } else { 1.0 };
    let active = Color::srgba(0.20, 0.35, 0.60, alpha);
    let inactive = Color::srgba(0.10, 0.10, 0.16, alpha);

    let (daily_color, minute_color) = match params.granularity {
        GranularityChoice::Daily => (active, inactive),
        GranularityChoice::Minute => (inactive, active),
    };

    for mut bg in daily_bg_q.iter_mut() {
        *bg = BackgroundColor(daily_color);
    }
    for mut bg in minute_bg_q.iter_mut() {
        *bg = BackgroundColor(minute_color);
    }

    for (label, mut text) in label_q.iter_mut() {
        let msg = match label.field {
            ScenarioStartupField::Start => params.errors.start.as_deref(),
            ScenarioStartupField::End => params.errors.end.as_deref(),
            ScenarioStartupField::Granularity => params.errors.granularity.as_deref(),
            ScenarioStartupField::InitialCash => params.errors.initial_cash.as_deref(),
            ScenarioStartupField::CrossField => params.errors.cross_field.as_deref(),
        };
        let new_str = msg.unwrap_or("");
        if text.0 != new_str {
            text.0 = new_str.to_string();
        }
    }
}

/// cache sidecar の `scenario.{start,end,granularity,initial_cash}` だけを置換する atomic write。
/// 既存 `rewrite_scenario_instruments_atomic` (src/ui/components.rs) と同じ tmp file 命名規約を採用。
fn rewrite_scenario_startup_params_atomic(
    path: &std::path::Path,
    start: &str,
    end: &str,
    granularity: &str,
    initial_cash: &str,
) -> std::io::Result<()> {
    let raw = crate::ui::layout_persistence::read_json_with_bom_strip(path)?;
    let mut value: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    {
        let scenario = value
            .get_mut("scenario")
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "missing scenario object")
            })?;

        let start_v = if start.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::Value::String(start.to_string())
        };
        let end_v = if end.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::Value::String(end.to_string())
        };
        let cash_v = match initial_cash.parse::<i64>() {
            Ok(n) => serde_json::Value::Number(n.into()),
            Err(_) => serde_json::Value::Null,
        };

        scenario.insert("start".to_string(), start_v);
        scenario.insert("end".to_string(), end_v);
        scenario.insert(
            "granularity".to_string(),
            serde_json::Value::String(granularity.to_string()),
        );
        scenario.insert("initial_cash".to_string(), cash_v);
    }

    let serialized = serde_json::to_string_pretty(&value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let dir = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent")
    })?;
    let file_name = path.file_name().and_then(|s| s.to_str()).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no filename")
    })?;
    let tmp = dir.join(format!(
        ".{}.tmp-{}-{}",
        file_name,
        std::process::id(),
        rand::random::<u32>()
    ));
    std::fs::write(&tmp, serialized.as_bytes())?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn granularity_as_canonical_str_returns_pascal_case() {
        assert_eq!(GranularityChoice::Daily.as_canonical_str(), "Daily");
        assert_eq!(GranularityChoice::Minute.as_canonical_str(), "Minute");
    }

    #[test]
    fn granularity_parse_canonical_accepts_canonical_strings() {
        assert_eq!(
            GranularityChoice::parse_canonical("Daily"),
            Some(GranularityChoice::Daily)
        );
        assert_eq!(
            GranularityChoice::parse_canonical("Minute"),
            Some(GranularityChoice::Minute)
        );
    }

    #[test]
    fn granularity_parse_canonical_rejects_non_canonical_strings() {
        assert!(GranularityChoice::parse_canonical("daily").is_none());
        assert!(GranularityChoice::parse_canonical("minute").is_none());
        assert!(GranularityChoice::parse_canonical(" Daily ").is_none());
        assert!(GranularityChoice::parse_canonical("DAILY").is_none());
        assert!(GranularityChoice::parse_canonical("").is_none());
        assert!(GranularityChoice::parse_canonical("Tick").is_none());
    }

    #[test]
    fn granularity_canonical_roundtrip() {
        for choice in [GranularityChoice::Daily, GranularityChoice::Minute] {
            assert_eq!(
                GranularityChoice::parse_canonical(choice.as_canonical_str()),
                Some(choice)
            );
        }
    }

    fn make_app() -> App {
        let mut app = App::new();
        app.init_resource::<ScenarioMetadata>()
            .init_resource::<ScenarioStartupParams>()
            .init_resource::<ReplayStartupProgress>()
            .add_event::<ScenarioStartupParamCommit>()
            .add_systems(
                Update,
                (
                    sync_startup_params_from_scenario_system,
                    commit_startup_params_to_scenario_system,
                ),
            );
        app
    }

    /// #9 scenario params sync: ScenarioMetadata の各 field が
    /// ScenarioStartupParams に反映され、granularity error は消える。
    #[test]
    fn test_9_scenario_params_sync() {
        let mut app = make_app();
        {
            let mut meta = app.world_mut().resource_mut::<ScenarioMetadata>();
            meta.start = Some("2024-01-01".into());
            meta.end = Some("2024-02-01".into());
            meta.granularity = Some("Minute".into());
            meta.initial_cash = Some(100_000);
        }
        app.update();

        let params = app.world().resource::<ScenarioStartupParams>();
        assert_eq!(params.start, "2024-01-01");
        assert_eq!(params.end, "2024-02-01");
        assert_eq!(params.granularity, GranularityChoice::Minute);
        assert_eq!(params.initial_cash, "100000");
        assert!(params.errors.granularity.is_none());
    }

    /// #9b granularity Some("Tick") sync: 未知 granularity は default に
    /// fallback しつつ errors.granularity を立てる。
    #[test]
    fn test_9b_granularity_unknown_string() {
        let mut app = make_app();
        {
            let mut meta = app.world_mut().resource_mut::<ScenarioMetadata>();
            meta.granularity = Some("Tick".into());
        }
        app.update();

        let params = app.world().resource::<ScenarioStartupParams>();
        assert_eq!(params.granularity, GranularityChoice::default());
        let msg = params.errors.granularity.as_deref().unwrap_or("");
        assert!(msg.contains("Tick"), "error msg should mention 'Tick': {}", msg);
        assert!(msg.contains("Daily") && msg.contains("Minute"));
    }

    /// #9c granularity None sync: prompt メッセージが立つ。
    #[test]
    fn test_9c_granularity_none() {
        let mut app = make_app();
        // ScenarioMetadata::default() => granularity = None
        app.update();

        let params = app.world().resource::<ScenarioStartupParams>();
        assert_eq!(params.granularity, GranularityChoice::default());
        assert_eq!(
            params.errors.granularity.as_deref(),
            Some("Please select a granularity to enable Run")
        );
    }

    /// #11 validation failure: 不正な start commit で errors.start が立ち、
    /// ScenarioMetadata.start は更新されない、dirty/writeback_pending も触らない。
    #[test]
    fn test_11_validation_failure() {
        let mut app = make_app();
        // dirty = true にして sync を抑止し、commit 単独の挙動を見る
        {
            let mut params = app.world_mut().resource_mut::<ScenarioStartupParams>();
            params.dirty = true;
        }
        app.world_mut()
            .send_event(ScenarioStartupParamCommit::Start("not-a-date".into()));
        app.update();

        let params = app.world().resource::<ScenarioStartupParams>();
        let meta = app.world().resource::<ScenarioMetadata>();
        assert!(params.errors.start.is_some());
        assert!(meta.start.is_none());
        assert!(params.dirty, "dirty must remain true on validation failure");
        assert!(!params.writeback_pending);
    }

    /// #11b multiple field errors independent: start を直しても end error は残る。
    #[test]
    fn test_11b_multiple_field_errors_independent() {
        let mut app = make_app();
        {
            let mut params = app.world_mut().resource_mut::<ScenarioStartupParams>();
            params.dirty = true;
        }
        app.world_mut()
            .send_event(ScenarioStartupParamCommit::Start("bad".into()));
        app.world_mut()
            .send_event(ScenarioStartupParamCommit::End("also-bad".into()));
        app.update();
        {
            let params = app.world().resource::<ScenarioStartupParams>();
            assert!(params.errors.start.is_some());
            assert!(params.errors.end.is_some());
        }

        // Fix start only
        app.world_mut()
            .send_event(ScenarioStartupParamCommit::Start("2024-01-01".into()));
        app.update();

        let params = app.world().resource::<ScenarioStartupParams>();
        assert!(params.errors.start.is_none());
        assert!(
            params.errors.end.is_some(),
            "end error should remain independent"
        );
    }

    /// #10d disabled while progress visible:
    /// dirty / writeback_pending / errors を一切触らない。
    #[test]
    fn test_10d_disabled_while_progress_visible() {
        let mut app = make_app();
        {
            let mut progress = app.world_mut().resource_mut::<ReplayStartupProgress>();
            progress.visible = true;
        }
        {
            let mut params = app.world_mut().resource_mut::<ScenarioStartupParams>();
            params.dirty = true;
            params.writeback_pending = false;
            params.errors.start = None;
        }
        app.world_mut()
            .send_event(ScenarioStartupParamCommit::Start("2024-01-01".into()));
        app.update();

        let params = app.world().resource::<ScenarioStartupParams>();
        let meta = app.world().resource::<ScenarioMetadata>();
        assert!(params.errors.start.is_none(), "errors must not be touched");
        assert!(params.dirty, "dirty must not be flipped");
        assert!(!params.writeback_pending, "writeback_pending must not flip");
        assert!(meta.start.is_none(), "metadata must not be mutated");
    }

    /// #12 cache unavailable (compile-only, sync still works):
    /// cache sidecar I/O は I2b 以降に後回しなので、ここでは sync が
    /// 走ることだけ確認する（cache 経路はそもそも触っていない）。
    #[test]
    fn test_12_cache_unavailable_compile_only() {
        let mut app = make_app();
        {
            let mut meta = app.world_mut().resource_mut::<ScenarioMetadata>();
            meta.start = Some("2024-01-01".into());
            meta.granularity = Some("Daily".into());
        }
        app.update();

        let params = app.world().resource::<ScenarioStartupParams>();
        assert_eq!(params.start, "2024-01-01");
        assert_eq!(params.granularity, GranularityChoice::Daily);
    }

    // ─────────────────────────────────────────────────────────────────────
    // I2b: cache sidecar writeback tests
    // ─────────────────────────────────────────────────────────────────────

    use crate::ui::components::{
        InstrumentRegistry, ScenarioFileWatchState, ScenarioInstrumentsWritebackState,
        ScenarioReadTarget, ScenarioWritebackPaths, writeback_scenario_instruments_system,
    };
    use crate::ui::scenario_parser::parse_scenario_system;
    use std::fs;

    fn make_writeback_app(cache_path: std::path::PathBuf) -> App {
        let mut app = App::new();
        app.init_resource::<ScenarioMetadata>()
            .init_resource::<ScenarioStartupParams>()
            .init_resource::<ReplayStartupProgress>()
            .insert_resource(ScenarioWritebackPaths {
                cache_sidecar: Some(cache_path),
            })
            .add_event::<ScenarioStartupParamCommit>()
            .add_systems(Update, write_startup_params_to_cache_sidecar_system);
        app
    }

    /// #10 scenario params writeback to cache sidecar:
    /// writeback_pending=true で 1 tick 回すと cache.json の
    /// scenario.{start,end,granularity,initial_cash} だけが更新され、
    /// instruments / schema_version / unknown_field / layout は不変。
    #[test]
    fn test_10_scenario_params_writeback_to_cache_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("cache.json");
        let initial = r#"{
  "scenario": {
    "schema_version": 2,
    "instruments": ["1301.TSE"],
    "start": "2020-01-01",
    "unknown_field": "keep me"
  },
  "layout": {"foo": "bar"}
}"#;
        fs::write(&cache_path, initial).unwrap();

        let mut app = make_writeback_app(cache_path.clone());
        {
            let mut params = app.world_mut().resource_mut::<ScenarioStartupParams>();
            params.start = "2024-05-01".into();
            params.end = "2024-06-01".into();
            params.granularity = GranularityChoice::Minute;
            params.initial_cash = "500000".into();
            params.writeback_pending = true;
        }
        app.update();

        let params = app.world().resource::<ScenarioStartupParams>();
        assert!(!params.writeback_pending, "writeback_pending should clear");

        let written = fs::read_to_string(&cache_path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&written).unwrap();
        let scenario = v.get("scenario").unwrap();
        assert_eq!(scenario.get("start").unwrap().as_str(), Some("2024-05-01"));
        assert_eq!(scenario.get("end").unwrap().as_str(), Some("2024-06-01"));
        assert_eq!(
            scenario.get("granularity").unwrap().as_str(),
            Some("Minute")
        );
        assert_eq!(
            scenario.get("initial_cash").unwrap().as_i64(),
            Some(500_000)
        );
        // unchanged keys
        let instruments = scenario.get("instruments").unwrap().as_array().unwrap();
        assert_eq!(instruments.len(), 1);
        assert_eq!(instruments[0].as_str(), Some("1301.TSE"));
        assert_eq!(
            scenario.get("unknown_field").unwrap().as_str(),
            Some("keep me")
        );
        assert_eq!(scenario.get("schema_version").unwrap().as_u64(), Some(2));
        assert_eq!(v.get("layout").unwrap().get("foo").unwrap().as_str(), Some("bar"));
    }

    /// #10b round-trip via parse_scenario_system:
    /// write → metadata reset → ScenarioReadTarget セット → parse 1 tick で
    /// metadata.start / granularity が回復し、次 tick sync で
    /// params.granularity == Minute に戻る。
    #[test]
    fn test_10b_round_trip_via_parse_scenario_system() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("cache.json");
        let initial = r#"{"scenario": {"schema_version": 2, "instruments": ["1301.TSE"], "start": "2020-01-01", "unknown_field": "keep me"}}"#;
        fs::write(&cache_path, initial).unwrap();

        let mut app = make_writeback_app(cache_path.clone());
        {
            let mut params = app.world_mut().resource_mut::<ScenarioStartupParams>();
            params.start = "2024-05-01".into();
            params.end = "2024-06-01".into();
            params.granularity = GranularityChoice::Minute;
            params.initial_cash = "500000".into();
            params.writeback_pending = true;
        }
        app.update();

        // Re-parse via parse_scenario_system. Use the canonical system to ensure
        // the writeback output is consumable by the real parser.
        app.insert_resource(ScenarioMetadata::default());
        app.init_resource::<ScenarioFileWatchState>();
        app.insert_resource(ScenarioReadTarget(Some(cache_path.clone())));
        app.add_event::<crate::ui::components::ScenarioLoadedFromFile>();
        app.add_event::<crate::ui::components::ScenarioClearedFromFile>();
        app.add_systems(
            Update,
            (
                parse_scenario_system,
                sync_startup_params_from_scenario_system,
            )
                .chain(),
        );

        // First tick: parse_scenario_system reads cache, populates metadata;
        // sync also runs but params.dirty may be false → reflects metadata too.
        app.update();

        let meta = app.world().resource::<ScenarioMetadata>();
        assert_eq!(meta.start.as_deref(), Some("2024-05-01"));
        assert_eq!(meta.end.as_deref(), Some("2024-06-01"));
        assert_eq!(meta.granularity.as_deref(), Some("Minute"));
        assert_eq!(meta.initial_cash, Some(500_000));

        // After another tick the sync system has surely propagated.
        app.update();
        let params = app.world().resource::<ScenarioStartupParams>();
        assert_eq!(params.granularity, GranularityChoice::Minute);
    }

    /// #10c concurrent writeback with scenario.instruments:
    /// 同 cache.json に対し、instruments writeback と startup params writeback が
    /// 同 chain で連続して走り、両方の変更が共存する。
    #[test]
    fn test_10c_concurrent_writeback_with_instruments() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("cache.json");
        let initial = r#"{"scenario": {"schema_version": 2, "instruments": ["OLD.T"], "start": "2020-01-01", "granularity": "Daily", "unknown_field": "keep me"}}"#;
        fs::write(&cache_path, initial).unwrap();

        let mut app = App::new();
        app.init_resource::<ScenarioMetadata>()
            .init_resource::<ScenarioStartupParams>()
            .init_resource::<ReplayStartupProgress>()
            .init_resource::<InstrumentRegistry>()
            .init_resource::<ScenarioInstrumentsWritebackState>()
            .init_resource::<ScenarioFileWatchState>()
            .insert_resource(ScenarioReadTarget(Some(cache_path.clone())))
            .insert_resource(ScenarioWritebackPaths {
                cache_sidecar: Some(cache_path.clone()),
            })
            .add_event::<ScenarioStartupParamCommit>()
            .add_systems(
                Update,
                (
                    writeback_scenario_instruments_system,
                    write_startup_params_to_cache_sidecar_system,
                )
                    .chain(),
            );

        // Mark registry dirty with new ids
        {
            let mut registry = app.world_mut().resource_mut::<InstrumentRegistry>();
            registry.editable = true;
            registry.replace_all(&["NEW1.T".to_string(), "NEW2.T".to_string()]);
        }
        {
            let mut wb = app
                .world_mut()
                .resource_mut::<ScenarioInstrumentsWritebackState>();
            wb.revision += 1;
        }
        // Stage new startup params for writeback
        {
            let mut params = app.world_mut().resource_mut::<ScenarioStartupParams>();
            params.start = "2024-05-01".into();
            params.end = "2024-06-01".into();
            params.granularity = GranularityChoice::Minute;
            params.initial_cash = "500000".into();
            params.writeback_pending = true;
        }

        app.update();

        let written = fs::read_to_string(&cache_path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&written).unwrap();
        let scenario = v.get("scenario").unwrap();
        let instruments = scenario.get("instruments").unwrap().as_array().unwrap();
        assert_eq!(instruments.len(), 2);
        assert_eq!(instruments[0].as_str(), Some("NEW1.T"));
        assert_eq!(instruments[1].as_str(), Some("NEW2.T"));
        assert_eq!(scenario.get("start").unwrap().as_str(), Some("2024-05-01"));
        assert_eq!(scenario.get("end").unwrap().as_str(), Some("2024-06-01"));
        assert_eq!(
            scenario.get("granularity").unwrap().as_str(),
            Some("Minute")
        );
        assert_eq!(
            scenario.get("initial_cash").unwrap().as_i64(),
            Some(500_000)
        );
        assert_eq!(
            scenario.get("unknown_field").unwrap().as_str(),
            Some("keep me")
        );
    }
}
