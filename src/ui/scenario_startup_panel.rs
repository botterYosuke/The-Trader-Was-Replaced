use bevy::prelude::*;
use bevy_cosmic_edit::cosmic_text::{Attrs, AttrsOwned, Metrics};
use bevy_cosmic_edit::prelude::*;
use bevy_cosmic_edit::{
    CosmicBackgroundColor, CosmicRenderScale, CosmicTextAlign, CursorColor, ReadOnly,
    ScrollEnabled,
};
use chrono::NaiveDate;

use crate::replay::startup_progress::ReplayStartupProgress;
use bevy_cosmic_edit::CosmicTextChanged;

/// `BufferExtras::get_text` lives in a private cosmic-edit module, so we
/// re-join `Buffer.lines` manually.
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
    atomic_mutate_scenario_object,
};

const DATE_FMT: &str = "%Y-%m-%d";

const PANEL_BG: Color = Color::srgba(0.07, 0.07, 0.11, 1.0);
const PANEL_BORDER: Color = Color::srgba(0.18, 0.18, 0.28, 1.0);
const LABEL_COLOR: Color = Color::srgb(0.78, 0.82, 0.92);
const HEADER_COLOR: Color = Color::srgb(0.50, 0.70, 1.00);
const BTN_BG: Color = Color::srgba(0.10, 0.10, 0.16, 1.0);
const BTN_TEXT: Color = Color::srgb(0.78, 0.82, 0.92);
const ERROR_COLOR: Color = Color::srgb(0.95, 0.45, 0.45);
const LABEL_WIDTH: f32 = 70.0;
const FIELD_BG_ACTIVE: Color = Color::srgba(0.02, 0.02, 0.04, 1.0);
const FIELD_BG_DISABLED: Color = Color::srgba(0.08, 0.08, 0.10, 1.0);

impl GranularityChoice {
    /// `ScenarioMetadata.granularity` / cache sidecar JSON / `StrategyRunConfig.granularity`
    /// must use this exact spelling.
    pub fn as_canonical_str(self) -> &'static str {
        match self {
            GranularityChoice::Daily => "Daily",
            GranularityChoice::Minute => "Minute",
        }
    }

    /// Exact-match parse: "daily" / " Daily " / "DAILY" all return `None`.
    pub fn parse_canonical(s: &str) -> Option<Self> {
        match s {
            "Daily" => Some(GranularityChoice::Daily),
            "Minute" => Some(GranularityChoice::Minute),
            _ => None,
        }
    }
}

/// The panel must reject edits while a replay is starting and also when no
/// cache sidecar is configured: without a writeback target, accepting input
/// would leave `writeback_pending` permanently stuck and silently diverge
/// in-memory `ScenarioMetadata` from disk.
fn is_panel_disabled(progress: &ReplayStartupProgress, paths: &ScenarioWritebackPaths) -> bool {
    progress.visible || paths.cache_sidecar.is_none()
}

/// Validate one of the date input fields. `field_name` is used to build a
/// human-readable "{field_name} must not be empty" error.
fn validate_date_field(field_name: &str, s: &str) -> Result<NaiveDate, String> {
    if s.is_empty() {
        return Err(format!("{} must not be empty", field_name));
    }
    NaiveDate::parse_from_str(s, DATE_FMT)
        .map_err(|_| format!("invalid date '{}'; use YYYY-MM-DD", s))
}

#[derive(Event, Debug, Clone)]
pub enum ScenarioStartupParamCommit {
    Start(String),
    End(String),
    Granularity(GranularityChoice),
    InitialCash(String),
}

fn spawn_text_field_row(
    panel: &mut ChildBuilder,
    label: &str,
    host_marker: impl Bundle,
    error_field: ScenarioStartupField,
) {
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
                Text::new(label),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(LABEL_COLOR),
                Node {
                    width: Val::Px(LABEL_WIDTH),
                    ..default()
                },
            ));
            row.spawn((
                Node {
                    flex_grow: 1.0,
                    // Must be tall enough to hold one line of cosmic-edit
                    // text after DPI scaling (set_initial_scale doubles the
                    // initial Metrics on a 2x DPI display, so line_height
                    // goes from 7 to 14). A row shorter than the scaled
                    // line_height produces 0 layout runs and no glyphs.
                    height: Val::Px(16.0),
                    ..default()
                },
                host_marker,
            ));
        });
    panel.spawn((
        Text::new(""),
        TextFont {
            font_size: 10.0,
            ..default()
        },
        TextColor(ERROR_COLOR),
        Node {
            padding: UiRect::left(Val::Px(LABEL_WIDTH)),
            ..default()
        },
        ScenarioStartupErrorLabel { field: error_field },
    ));
}

fn spawn_granularity_button(row: &mut ChildBuilder, label: &str, marker: impl Bundle, gap_right: bool) {
    let margin = if gap_right {
        UiRect::right(Val::Px(2.0))
    } else {
        UiRect::default()
    };
    row.spawn((
        Button,
        Node {
            padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
            margin,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(BTN_BG),
        marker,
    ))
    .with_children(|btn| {
        btn.spawn((
            Text::new(label),
            TextFont {
                font_size: 10.0,
                ..default()
            },
            TextColor(BTN_TEXT),
        ));
    });
}

pub fn spawn_scenario_startup_panel(
    mut commands: Commands,
    sidebar_q: Query<Entity, With<SidebarRoot>>,
) {
    let Ok(sidebar) = sidebar_q.get_single() else {
        return;
    };

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
                BackgroundColor(PANEL_BG),
                BorderColor(PANEL_BORDER),
                ScenarioStartupPanelRoot,
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new("Startup"),
                    TextFont {
                        font_size: 10.0,
                        ..default()
                    },
                    TextColor(HEADER_COLOR),
                    Node {
                        padding: UiRect::axes(Val::Px(2.0), Val::Px(2.0)),
                        ..default()
                    },
                ));

                spawn_text_field_row(
                    panel,
                    "Start",
                    ScenarioStartupStartFieldHost,
                    ScenarioStartupField::Start,
                );
                spawn_text_field_row(
                    panel,
                    "End",
                    ScenarioStartupEndFieldHost,
                    ScenarioStartupField::End,
                );

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
                            TextColor(LABEL_COLOR),
                            Node {
                                width: Val::Px(LABEL_WIDTH),
                                ..default()
                            },
                        ));
                        spawn_granularity_button(
                            row,
                            "Daily",
                            ScenarioStartupGranularityDailyButton,
                            true,
                        );
                        spawn_granularity_button(
                            row,
                            "Minute",
                            ScenarioStartupGranularityMinuteButton,
                            false,
                        );
                    });
                panel.spawn((
                    Text::new(""),
                    TextFont {
                        font_size: 10.0,
                        ..default()
                    },
                    TextColor(ERROR_COLOR),
                    Node {
                        padding: UiRect::left(Val::Px(LABEL_WIDTH)),
                        ..default()
                    },
                    ScenarioStartupErrorLabel {
                        field: ScenarioStartupField::Granularity,
                    },
                ));

                spawn_text_field_row(
                    panel,
                    "Initial cash",
                    ScenarioStartupCashFieldHost,
                    ScenarioStartupField::InitialCash,
                );

                panel.spawn((
                    Text::new(""),
                    TextFont {
                        font_size: 10.0,
                        ..default()
                    },
                    TextColor(ERROR_COLOR),
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

/// Attach a cosmic-edit `TextEdit` to each field host. Focus is handled by
/// `change_active_editor_ui` on click; `FocusedWidget` is intentionally untouched.
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
        let text_attrs = Attrs::new().color(CosmicColor::rgb(220, 220, 220));
        let entity = commands
            .spawn((
                TextEdit,
                CosmicEditBuffer::new(font_system, Metrics::new(5.5, 7.0))
                    .with_text(font_system, "", text_attrs),
                // render_texture reads font_color from DefaultAttrs (not from
                // the Attrs passed to set_text). Without this, font_color
                // falls back to rgb(0,0,0) and the text becomes invisible on
                // the dark background even though the buffer holds the value.
                DefaultAttrs(AttrsOwned::new(text_attrs)),
                CursorColor(Color::WHITE),
                CosmicBackgroundColor(FIELD_BG_ACTIVE),
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

/// One-way sync from ScenarioMetadata into ScenarioStartupParams.
///
/// - skip while the progress window is visible (run-in-progress)
/// - skip while the user is mid-edit (`params.dirty`)
/// - skip when metadata hasn't changed (avoid per-frame re-clones that mark
///   `ScenarioStartupParams` Changed and re-trigger the whole pipeline)
pub fn sync_startup_params_from_scenario_system(
    metadata: Res<ScenarioMetadata>,
    progress: Res<ReplayStartupProgress>,
    mut params: ResMut<ScenarioStartupParams>,
) {
    if progress.visible || params.dirty {
        return;
    }
    if !metadata.is_changed() {
        return;
    }

    params.start = metadata.start.clone().unwrap_or_default();
    params.end = metadata.end.clone().unwrap_or_default();

    match metadata.granularity.as_deref() {
        Some(s) => match GranularityChoice::parse_canonical(s) {
            Some(choice) => {
                params.granularity = choice;
                params.errors.granularity = None;
            }
            None => {
                params.granularity = GranularityChoice::default();
                params.errors.granularity = Some(format!(
                    "unknown granularity '{}'; please select Daily or Minute to enable Run",
                    s
                ));
            }
        },
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

pub fn commit_startup_params_to_scenario_system(
    mut events: EventReader<ScenarioStartupParamCommit>,
    mut params: ResMut<ScenarioStartupParams>,
    mut metadata: ResMut<ScenarioMetadata>,
    progress: Res<ReplayStartupProgress>,
    paths: Res<ScenarioWritebackPaths>,
) {
    if is_panel_disabled(&progress, &paths) {
        for _ in events.read() {}
        return;
    }

    let mut any_committed = false;
    let mut any_event = false;

    for ev in events.read() {
        any_event = true;
        match ev {
            ScenarioStartupParamCommit::Start(s) => match validate_date_field("start", s) {
                Err(msg) => {
                    params.errors.start = Some(msg);
                }
                Ok(_) => {
                    params.start = s.clone();
                    params.errors.start = None;
                    metadata.start = Some(s.clone());
                    any_committed = true;
                }
            },
            ScenarioStartupParamCommit::End(s) => match validate_date_field("end", s) {
                Err(msg) => {
                    params.errors.end = Some(msg);
                }
                Ok(_) => {
                    params.end = s.clone();
                    params.errors.end = None;
                    metadata.end = Some(s.clone());
                    any_committed = true;
                }
            },
            ScenarioStartupParamCommit::Granularity(g) => {
                params.granularity = *g;
                params.errors.granularity = None;
                metadata.granularity = Some(g.as_canonical_str().to_string());
                any_committed = true;
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
                    any_committed = true;
                }
            },
        }
    }

    if any_event {
        // Cross-field check only makes sense when both individual fields are
        // currently valid — otherwise the per-field error already conveys the
        // problem and a lingering "start must be on or before end" is misleading.
        let both_fields_valid =
            params.errors.start.is_none() && params.errors.end.is_none();
        let start_parsed = NaiveDate::parse_from_str(&params.start, DATE_FMT).ok();
        let end_parsed = NaiveDate::parse_from_str(&params.end, DATE_FMT).ok();
        params.errors.cross_field = match (both_fields_valid, start_parsed, end_parsed) {
            (true, Some(sd), Some(ed)) if sd > ed => {
                Some("start must be on or before end".into())
            }
            _ => None,
        };
    }

    // Only schedule a cache writeback when the batch leaves the form in a
    // fully valid state. Otherwise the cache would record a partial edit
    // (e.g. start updated but end invalid) — the plan requires that invalid
    // input never modifies the cache JSON.
    if any_committed && !params.errors.any() {
        params.dirty = false;
        params.writeback_pending = true;
    }
}

/// On `writeback_pending`, replace `scenario.{start,end,granularity,initial_cash}`
/// in the cache sidecar JSON atomically. Other keys (instruments, layout, unknown
/// fields) are preserved verbatim. Failure leaves `writeback_pending` set so the
/// next tick retries.
pub fn write_startup_params_to_cache_sidecar_system(
    mut params: ResMut<ScenarioStartupParams>,
    paths: Res<ScenarioWritebackPaths>,
    progress: Res<ReplayStartupProgress>,
) {
    if progress.visible || !params.writeback_pending {
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
            warn!("startup params writeback failed: {:?}: {}", path, e);
        }
    }
}

/// Propagate `params.{start,end,initial_cash}` strings into each cosmic-edit
/// buffer. Gated on `params.is_changed()` to avoid resetting the user's cursor.
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
        if buffer_text(&buffer) == expected {
            continue;
        }
        buffer.set_text(
            &mut font_system,
            expected,
            Attrs::new().color(CosmicColor::rgb(220, 220, 220)),
        );
    }
}

pub fn scenario_startup_param_input_system(
    mut events: EventReader<CosmicTextChanged>,
    editors_q: Query<&ScenarioStartupFieldEditor>,
    mut commit_w: EventWriter<ScenarioStartupParamCommit>,
    mut params: ResMut<ScenarioStartupParams>,
    progress: Res<ReplayStartupProgress>,
    paths: Res<ScenarioWritebackPaths>,
) {
    if is_panel_disabled(&progress, &paths) {
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
    paths: Res<ScenarioWritebackPaths>,
) {
    if is_panel_disabled(&progress, &paths) {
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

/// Toggle `ReadOnly` + dim the field background on the 3 CosmicEditor entities
/// while `is_panel_disabled`. The commit / writeback / input systems already
/// short-circuit on `is_panel_disabled`, but without this the editors still
/// accept clicks and key input visually, letting the user mutate the buffer
/// during a Run; the trailing `CosmicTextChanged` event then races with the
/// `progress.visible` flip and can leak through after auto-hide.
pub fn enforce_scenario_startup_panel_readonly_system(
    mut commands: Commands,
    progress: Res<ReplayStartupProgress>,
    paths: Res<ScenarioWritebackPaths>,
    mut q: Query<(
        Entity,
        Option<&ReadOnly>,
        &mut CosmicBackgroundColor,
    ), With<ScenarioStartupFieldEditor>>,
) {
    let disabled = is_panel_disabled(&progress, &paths);
    let target_bg = if disabled { FIELD_BG_DISABLED } else { FIELD_BG_ACTIVE };
    for (entity, ro, mut bg) in q.iter_mut() {
        match (disabled, ro.is_some()) {
            (true, false) => {
                commands.entity(entity).insert(ReadOnly);
            }
            (false, true) => {
                commands.entity(entity).remove::<ReadOnly>();
            }
            _ => {}
        }
        if bg.0 != target_bg {
            bg.0 = target_bg;
        }
    }
}

/// Repaint granularity button highlight + error labels.
///
/// Writes through `BackgroundColor`/`Text` are guarded with `!= new` to avoid
/// firing `Changed<T>` every frame.
pub fn update_scenario_startup_param_ui_system(
    params: Res<ScenarioStartupParams>,
    progress: Res<ReplayStartupProgress>,
    paths: Res<ScenarioWritebackPaths>,
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
    let alpha = if is_panel_disabled(&progress, &paths) {
        0.5
    } else {
        1.0
    };
    let active = Color::srgba(0.20, 0.35, 0.60, alpha);
    let inactive = Color::srgba(0.10, 0.10, 0.16, alpha);

    let (daily_color, minute_color) = match params.granularity {
        GranularityChoice::Daily => (active, inactive),
        GranularityChoice::Minute => (inactive, active),
    };

    for mut bg in daily_bg_q.iter_mut() {
        if bg.0 != daily_color {
            bg.0 = daily_color;
        }
    }
    for mut bg in minute_bg_q.iter_mut() {
        if bg.0 != minute_color {
            bg.0 = minute_color;
        }
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

fn rewrite_scenario_startup_params_atomic(
    path: &std::path::Path,
    start: &str,
    end: &str,
    granularity: &str,
    initial_cash: &str,
) -> std::io::Result<()> {
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
    let granularity = granularity.to_string();

    atomic_mutate_scenario_object(path, move |scenario| {
        scenario.insert("start".to_string(), start_v);
        scenario.insert("end".to_string(), end_v);
        scenario.insert(
            "granularity".to_string(),
            serde_json::Value::String(granularity),
        );
        scenario.insert("initial_cash".to_string(), cash_v);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Finding A (E2E §13 regression): the visible-flag short-circuit on the
    /// commit/input/writeback systems is necessary but not sufficient — without
    /// also flipping the editors to ReadOnly the user can still mutate the
    /// CosmicEditor buffer during a Run, and the trailing CosmicTextChanged
    /// event can leak through after `progress.visible` returns to false.
    #[test]
    fn enforce_readonly_toggles_with_progress_visible() {
        let mut app = App::new();
        app.init_resource::<ReplayStartupProgress>()
            .insert_resource(ScenarioWritebackPaths {
                cache_sidecar: Some(std::path::PathBuf::from("/tmp/dummy_cache.json")),
            })
            .add_systems(Update, enforce_scenario_startup_panel_readonly_system);

        let e = app
            .world_mut()
            .spawn((
                ScenarioStartupFieldEditor {
                    field: ScenarioStartupField::InitialCash,
                },
                CosmicBackgroundColor(FIELD_BG_ACTIVE),
            ))
            .id();

        // Run while progress hidden — editor must be editable.
        app.update();
        assert!(app.world().get::<ReadOnly>(e).is_none());
        assert_eq!(
            app.world().get::<CosmicBackgroundColor>(e).unwrap().0,
            FIELD_BG_ACTIVE
        );

        // Flip to visible — editor must become ReadOnly + dimmed.
        app.world_mut()
            .resource_mut::<ReplayStartupProgress>()
            .visible = true;
        app.update();
        assert!(app.world().get::<ReadOnly>(e).is_some());
        assert_eq!(
            app.world().get::<CosmicBackgroundColor>(e).unwrap().0,
            FIELD_BG_DISABLED
        );

        // Flip back — marker must be removed and bg restored.
        app.world_mut()
            .resource_mut::<ReplayStartupProgress>()
            .visible = false;
        app.update();
        assert!(app.world().get::<ReadOnly>(e).is_none());
        assert_eq!(
            app.world().get::<CosmicBackgroundColor>(e).unwrap().0,
            FIELD_BG_ACTIVE
        );
    }

    /// Cache-unavailable (`ScenarioWritebackPaths.cache_sidecar = None`) also
    /// disables the panel per plan §"cache_sidecar == None" — the readonly
    /// enforcement must mirror that, not only watch `progress.visible`.
    #[test]
    fn enforce_readonly_when_cache_unavailable() {
        let mut app = App::new();
        app.init_resource::<ReplayStartupProgress>()
            .insert_resource(ScenarioWritebackPaths { cache_sidecar: None })
            .add_systems(Update, enforce_scenario_startup_panel_readonly_system);

        let e = app
            .world_mut()
            .spawn((
                ScenarioStartupFieldEditor {
                    field: ScenarioStartupField::Start,
                },
                CosmicBackgroundColor(FIELD_BG_ACTIVE),
            ))
            .id();

        app.update();
        assert!(app.world().get::<ReadOnly>(e).is_some());
        assert_eq!(
            app.world().get::<CosmicBackgroundColor>(e).unwrap().0,
            FIELD_BG_DISABLED
        );
    }

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
            // A dummy cache path keeps the panel enabled; tests that exercise
            // the cache-unavailable path override this with `cache_sidecar:
            // None`.
            .insert_resource(ScenarioWritebackPaths {
                cache_sidecar: Some(std::path::PathBuf::from("/tmp/dummy_cache.json")),
            })
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

    /// #9 sub-spec: while the user is mid-edit (`params.dirty == true`),
    /// a `ScenarioMetadata` change must NOT clobber the in-flight UI strings.
    /// Without this guard, every metadata reparse during typing would snap
    /// the input field back to the parsed value.
    #[test]
    fn test_9_sync_skipped_while_dirty() {
        let mut app = make_app();
        {
            let mut params = app.world_mut().resource_mut::<ScenarioStartupParams>();
            params.dirty = true;
            params.start = "2099-12-31".into();
        }
        {
            let mut meta = app.world_mut().resource_mut::<ScenarioMetadata>();
            meta.start = Some("2024-01-01".into());
            meta.granularity = Some("Daily".into());
        }
        app.update();

        let params = app.world().resource::<ScenarioStartupParams>();
        assert_eq!(
            params.start, "2099-12-31",
            "in-flight UI value must not be overwritten by metadata sync while dirty"
        );
    }

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

    #[test]
    fn test_9c_granularity_none() {
        let mut app = make_app();
        app.update();

        let params = app.world().resource::<ScenarioStartupParams>();
        assert_eq!(params.granularity, GranularityChoice::default());
        assert_eq!(
            params.errors.granularity.as_deref(),
            Some("Please select a granularity to enable Run")
        );
    }

    #[test]
    fn test_11_validation_failure() {
        let mut app = make_app();
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

    /// #12: when `ScenarioWritebackPaths.cache_sidecar == None`, the writeback
    /// system must early-return without touching `writeback_pending` (so no
    /// retry loop fires) and without panicking. Run itself is not blocked by
    /// the missing cache path — `errors.any()` stays `false`.
    #[test]
    fn test_12_cache_unavailable_writeback_is_noop() {
        let mut app = App::new();
        app.init_resource::<ScenarioMetadata>()
            .init_resource::<ScenarioStartupParams>()
            .init_resource::<ReplayStartupProgress>()
            .insert_resource(ScenarioWritebackPaths {
                cache_sidecar: None,
            })
            .add_systems(Update, write_startup_params_to_cache_sidecar_system);

        {
            let mut params = app.world_mut().resource_mut::<ScenarioStartupParams>();
            params.start = "2024-05-01".into();
            params.end = "2024-06-01".into();
            params.granularity = GranularityChoice::Daily;
            params.initial_cash = "1000".into();
            params.writeback_pending = true;
        }

        // Should not panic, should not flip writeback_pending (no path to write to).
        app.update();

        let params = app.world().resource::<ScenarioStartupParams>();
        assert!(
            params.writeback_pending,
            "writeback_pending must stay true when cache_sidecar is None (no path to flush to)"
        );
        assert!(
            !params.errors.any(),
            "missing cache sidecar must not flip into a Run-blocking error"
        );
    }

    /// When a batch commits successful fields but leaves any error (e.g. a
    /// cross-field violation, or a sibling field that failed validation), the
    /// cache writeback must NOT be scheduled — otherwise the cache JSON would
    /// record a partial edit that contradicts the on-screen error.
    #[test]
    fn writeback_pending_not_set_when_batch_leaves_errors() {
        let mut app = make_app();
        // Valid start, then end that's *before* start → cross_field fires.
        app.world_mut()
            .send_event(ScenarioStartupParamCommit::Start("2024-06-01".into()));
        app.world_mut()
            .send_event(ScenarioStartupParamCommit::End("2024-01-01".into()));
        app.update();

        let params = app.world().resource::<ScenarioStartupParams>();
        assert!(
            params.errors.cross_field.is_some(),
            "precondition: cross_field error should fire"
        );
        assert!(
            !params.writeback_pending,
            "writeback_pending must not be set while any error remains"
        );

        // Valid date + invalid cash → cash error remains, no writeback scheduled.
        let mut app = make_app();
        app.world_mut()
            .send_event(ScenarioStartupParamCommit::Start("2024-01-01".into()));
        app.world_mut()
            .send_event(ScenarioStartupParamCommit::InitialCash("not-a-number".into()));
        app.update();
        let params = app.world().resource::<ScenarioStartupParams>();
        assert!(params.errors.initial_cash.is_some());
        assert!(
            !params.writeback_pending,
            "partial-error batch must not request a writeback"
        );
    }

    /// When `cache_sidecar` is `None` the panel is effectively read-only: commit
    /// events drain without touching `ScenarioMetadata` or `params.dirty`. This
    /// prevents `writeback_pending` from getting permanently stuck (the writer
    /// system early-returns on missing path, so nothing would ever clear it).
    #[test]
    fn panel_disabled_when_cache_sidecar_unavailable() {
        let mut app = make_app();
        // Override make_app's dummy path with None.
        app.insert_resource(ScenarioWritebackPaths {
            cache_sidecar: None,
        });
        app.world_mut()
            .send_event(ScenarioStartupParamCommit::Start("2024-01-01".into()));
        app.update();

        let params = app.world().resource::<ScenarioStartupParams>();
        let meta = app.world().resource::<ScenarioMetadata>();
        assert!(meta.start.is_none(), "metadata must not mutate");
        assert!(!params.dirty, "dirty must stay false");
        assert!(!params.writeback_pending, "writeback_pending must stay false");
        assert!(params.errors.start.is_none(), "errors must stay untouched");
    }

    /// Cross-field error must clear when one date becomes invalid (otherwise
    /// the user sees stale "start must be on or before end" alongside a
    /// "invalid date" error).
    #[test]
    fn cross_field_error_clears_when_one_date_invalid() {
        let mut app = make_app();
        {
            let mut params = app.world_mut().resource_mut::<ScenarioStartupParams>();
            params.dirty = true;
        }
        // First set start > end so cross_field error fires.
        app.world_mut()
            .send_event(ScenarioStartupParamCommit::Start("2024-06-01".into()));
        app.world_mut()
            .send_event(ScenarioStartupParamCommit::End("2024-01-01".into()));
        app.update();
        assert!(app
            .world()
            .resource::<ScenarioStartupParams>()
            .errors
            .cross_field
            .is_some());

        // Now invalidate end; cross_field should no longer claim ordering.
        app.world_mut()
            .send_event(ScenarioStartupParamCommit::End("not-a-date".into()));
        app.update();

        let params = app.world().resource::<ScenarioStartupParams>();
        assert!(params.errors.end.is_some());
        assert!(
            params.errors.cross_field.is_none(),
            "cross_field must not linger when one date fails to parse"
        );
    }

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

        app.update();

        let meta = app.world().resource::<ScenarioMetadata>();
        assert_eq!(meta.start.as_deref(), Some("2024-05-01"));
        assert_eq!(meta.end.as_deref(), Some("2024-06-01"));
        assert_eq!(meta.granularity.as_deref(), Some("Minute"));
        assert_eq!(meta.initial_cash, Some(500_000));

        app.update();
        let params = app.world().resource::<ScenarioStartupParams>();
        assert_eq!(params.granularity, GranularityChoice::Minute);
    }

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
            // writeback_scenario_instruments_system は ExecutionModeRes を要求する（Replay gate）
            .insert_resource(crate::trading::ExecutionModeRes {
                mode: crate::trading::ExecutionMode::Replay,
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
