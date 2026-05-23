use bevy::prelude::*;
use bevy_cosmic_edit::cosmic_text::{Attrs, AttrsOwned, Metrics};
use bevy_cosmic_edit::prelude::*;
use bevy_cosmic_edit::{
    CosmicBackgroundColor, CosmicRenderScale, CosmicTextAlign, CosmicWrap, CursorColor, ReadOnly,
    ScrollEnabled,
};
use chrono::{Months, NaiveDate};

use crate::replay::startup_progress::ReplayStartupProgress;
use crate::ui::render_scale::RenderScaleResponsive;
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
    GranularityChoice, PanelKind, ScenarioMetadata, ScenarioStartupCashFieldHost,
    ScenarioStartupEndFieldHost, ScenarioStartupErrorLabel, ScenarioStartupField,
    ScenarioStartupFieldEditor, ScenarioStartupGranularityDailyButton,
    ScenarioStartupGranularityMinuteButton, ScenarioStartupPanelRoot, ScenarioStartupParams,
    ScenarioStartupStartFieldHost, ScenarioWritebackPaths,
    atomic_mutate_scenario_object,
};
use crate::ui::floating_window::{FloatingWindowSpec, spawn_floating_window};

const DATE_FMT: &str = "%Y-%m-%d";

const ERROR_COLOR: Color = Color::srgb(0.95, 0.45, 0.45);
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

/// #20: sidecar に値が無い初回のデフォルト日付レンジを返す純関数。
/// `(Start = today − 3 ヶ月, End = today)` を `DATE_FMT`(%Y-%m-%d) で文字列化する。
/// 月末跨ぎは `checked_sub_months` 仕様で対象月の末日へクランプされる。
fn default_date_range(today: NaiveDate) -> (String, String) {
    let start = today
        .checked_sub_months(Months::new(3))
        .unwrap_or(today);
    (
        start.format(DATE_FMT).to_string(),
        today.format(DATE_FMT).to_string(),
    )
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

/// world-space (sprite) STARTUP floating window. step 4 で旧 sidebar flexbox panel
/// を置き換える新経路。host marker / error-label
/// marker / granularity-button marker を既存システムが期待する数・種類で再現する。
/// Startup 時に sprite startup window を一度だけ spawn する system ラッパ。
/// 本体は helper `spawn_scenario_startup_window(&mut Commands)`（4d の dispatcher 復元 arm も同 helper を呼ぶ）。
pub fn spawn_scenario_startup_window_system(mut commands: Commands) {
    spawn_scenario_startup_window(&mut commands);
}

pub fn spawn_scenario_startup_window(commands: &mut Commands) {
    const WINDOW_SIZE: Vec2 = Vec2::new(320.0, 200.0);
    const WINDOW_POSITION: Vec2 = Vec2::new(-450.0, -120.0);
    const ACCENT: Color = Color::srgba(0.5, 0.7, 1.0, 0.4);

    // Labels are right-anchored, so LABEL_X is their RIGHT edge (10px left of
    // the field's left edge at FIELD_X-60 = -60). Right-aligning keeps the
    // label→field gap constant regardless of label length and stops long
    // labels ("Initial cash" / "Granularity") from overrunning the window's
    // left border into the panel behind it.
    const LABEL_X: f32 = -70.0;
    const FIELD_X: f32 = 0.0;
    const FIELD_SIZE: Vec2 = Vec2::new(120.0, 22.0);
    const ERROR_X: f32 = -40.0;
    const STARTUP_LABEL_COLOR: Color = Color::srgb(0.78, 0.82, 0.92);
    const FIELD_BG: Color = Color::srgba(0.02, 0.02, 0.04, 1.0);
    const GRAN_BTN_BG: Color = Color::srgba(0.10, 0.10, 0.16, 1.0);
    const GRAN_BTN_SIZE: Vec2 = Vec2::new(50.0, 16.0);

    let (root, content_area, _title_bar) = spawn_floating_window(
        commands,
        FloatingWindowSpec {
            title: "STARTUP".to_string(),
            size: WINDOW_SIZE,
            position: WINDOW_POSITION,
            accent: ACCENT,
            closeable: false,
            resizable: false,
        },
    );
    commands
        .entity(root)
        .insert((PanelKind::Startup, ScenarioStartupPanelRoot));

    // ── ラベル + フィールド host を 1 行 spawn する helper ──
    fn spawn_field_row(
        commands: &mut Commands,
        parent: Entity,
        y: f32,
        label: &str,
        host_marker: impl Bundle,
    ) {
        let lbl = commands
            .spawn((
                Text2d::new(label),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(STARTUP_LABEL_COLOR),
                bevy::sprite::Anchor::CenterRight,
                Transform::from_xyz(LABEL_X, y, 0.1),
            ))
            .id();
        commands.entity(parent).add_child(lbl);

        let host = commands
            .spawn((
                Sprite {
                    color: FIELD_BG,
                    custom_size: Some(FIELD_SIZE),
                    ..default()
                },
                Transform::from_xyz(FIELD_X, y, 0.1),
                host_marker,
            ))
            .id();
        commands.entity(parent).add_child(host);
    }

    // ── エラーラベル (空文字で spawn、update system が後で書く) helper ──
    fn spawn_error_label(
        commands: &mut Commands,
        parent: Entity,
        y: f32,
        field: ScenarioStartupField,
    ) {
        let err = commands
            .spawn((
                Text2d::new(""),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(ERROR_COLOR),
                Transform::from_xyz(ERROR_X, y, 0.1),
                ScenarioStartupErrorLabel { field },
            ))
            .id();
        commands.entity(parent).add_child(err);
    }

    // ── granularity ボタン (sprite + Transform + marker、子に Text2d ラベル) helper ──
    fn spawn_granularity_btn(
        commands: &mut Commands,
        parent: Entity,
        x: f32,
        y: f32,
        label: &str,
        marker: impl Bundle,
        choice: GranularityChoice,
    ) {
        let btn = commands
            .spawn((
                Sprite {
                    color: GRAN_BTN_BG,
                    custom_size: Some(GRAN_BTN_SIZE),
                    ..default()
                },
                Transform::from_xyz(x, y, 0.1),
                marker,
            ))
            .observe(
                move |_trigger: Trigger<Pointer<Click>>,
                      mut commit_w: EventWriter<ScenarioStartupParamCommit>,
                      mut params: ResMut<ScenarioStartupParams>,
                      progress: Res<ReplayStartupProgress>,
                      paths: Res<ScenarioWritebackPaths>| {
                    if is_panel_disabled(&progress, &paths) {
                        return;
                    }
                    commit_w.send(ScenarioStartupParamCommit::Granularity(choice));
                    params.dirty = true;
                },
            )
            .id();
        commands.entity(parent).add_child(btn);

        let txt = commands
            .spawn((
                Text2d::new(label),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(STARTUP_LABEL_COLOR),
                Transform::from_xyz(0.0, 0.0, 0.1),
            ))
            .id();
        commands.entity(btn).add_child(txt);
    }

    // (a) Start 行 + (b) Start エラー
    spawn_field_row(commands, content_area, 70.0, "Start", ScenarioStartupStartFieldHost);
    spawn_error_label(commands, content_area, 56.0, ScenarioStartupField::Start);

    // (c) End 行 + End エラー
    spawn_field_row(commands, content_area, 38.0, "End", ScenarioStartupEndFieldHost);
    spawn_error_label(commands, content_area, 24.0, ScenarioStartupField::End);

    // (d) Granularity ラベル + 2 ボタン + Granularity エラー
    let gran_label = commands
        .spawn((
            Text2d::new("Granularity"),
            TextFont {
                font_size: 11.0,
                ..default()
            },
            TextColor(STARTUP_LABEL_COLOR),
            bevy::sprite::Anchor::CenterRight,
            Transform::from_xyz(LABEL_X, 6.0, 0.1),
        ))
        .id();
    commands.entity(content_area).add_child(gran_label);
    spawn_granularity_btn(
        commands,
        content_area,
        -30.0,
        6.0,
        "Daily",
        ScenarioStartupGranularityDailyButton,
        GranularityChoice::Daily,
    );
    spawn_granularity_btn(
        commands,
        content_area,
        30.0,
        6.0,
        "Minute",
        ScenarioStartupGranularityMinuteButton,
        GranularityChoice::Minute,
    );
    spawn_error_label(commands, content_area, -8.0, ScenarioStartupField::Granularity);

    // (e) Initial cash 行 + エラー
    spawn_field_row(
        commands,
        content_area,
        -26.0,
        "Initial cash",
        ScenarioStartupCashFieldHost,
    );
    spawn_error_label(commands, content_area, -40.0, ScenarioStartupField::InitialCash);

    // (f) CrossField エラー
    spawn_error_label(commands, content_area, -58.0, ScenarioStartupField::CrossField);
}

/// Attach a cosmic-edit `TextEdit` to each field host. Focus is handled by
/// `change_active_editor_sprite` on click; `FocusedWidget` is intentionally untouched.
pub fn spawn_scenario_startup_input_fields(
    mut commands: Commands,
    mut font_system: ResMut<bevy_cosmic_edit::prelude::CosmicFontSystem>,
    start_host_q: Query<
        Entity,
        (
            With<ScenarioStartupStartFieldHost>,
            Without<ScenarioStartupFieldEditor>,
        ),
    >,
    end_host_q: Query<
        Entity,
        (
            With<ScenarioStartupEndFieldHost>,
            Without<ScenarioStartupFieldEditor>,
        ),
    >,
    cash_host_q: Query<
        Entity,
        (
            With<ScenarioStartupCashFieldHost>,
            Without<ScenarioStartupFieldEditor>,
        ),
    >,
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
                TextEdit2d,
                CosmicEditBuffer::new(font_system, Metrics::new(9.0, 11.0)).with_text(
                    font_system,
                    "",
                    text_attrs,
                ),
                // render_texture reads font_color from DefaultAttrs (not from
                // the Attrs passed to set_text). Without this, font_color
                // falls back to rgb(0,0,0) and the text becomes invisible on
                // the dark background even though the buffer holds the value.
                DefaultAttrs(AttrsOwned::new(text_attrs)),
                CursorColor(Color::WHITE),
                CosmicBackgroundColor(FIELD_BG_ACTIVE),
                CosmicRenderScale(1.0),
                RenderScaleResponsive::new(4.0),
                CosmicTextAlign::TopLeft { padding: 1 },
                ScrollEnabled::Disabled,
                CosmicWrap::InfiniteLine,
                Sprite {
                    custom_size: Some(Vec2::new(120.0, 22.0)),
                    color: Color::WHITE,
                    ..default()
                },
                Transform::from_xyz(0.0, 0.0, 0.1),
                ScenarioStartupFieldEditor { field },
            ))
            .id();
        commands.entity(host).add_child(entity);
    }

    if let Ok(host) = start_host_q.get_single() {
        spawn_field(
            &mut commands,
            &mut font_system,
            host,
            ScenarioStartupField::Start,
        );
    }
    if let Ok(host) = end_host_q.get_single() {
        spawn_field(
            &mut commands,
            &mut font_system,
            host,
            ScenarioStartupField::End,
        );
    }
    if let Ok(host) = cash_host_q.get_single() {
        spawn_field(
            &mut commands,
            &mut font_system,
            host,
            ScenarioStartupField::InitialCash,
        );
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

    let today = chrono::Local::now().date_naive();
    let (default_start, default_end) = default_date_range(today);
    params.start = metadata.start.clone().unwrap_or(default_start);
    params.end = metadata.end.clone().unwrap_or(default_end);

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
            params.errors.granularity = Some("Please select a granularity to enable Run".into());
        }
    }

    params.initial_cash = match metadata.initial_cash {
        Some(n) => n.to_string(),
        None => "1000000".to_string(),
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
            ScenarioStartupParamCommit::InitialCash(s) if s.is_empty() => {
                params.errors.initial_cash = Some("initial cash must not be empty".into());
            }
            ScenarioStartupParamCommit::InitialCash(s) => match s.parse::<i64>() {
                Err(_) => {
                    params.errors.initial_cash = Some("invalid integer".into());
                }
                Ok(n) if n <= 0 => {
                    params.errors.initial_cash = Some("initial cash must be positive".into());
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
        let both_fields_valid = params.errors.start.is_none() && params.errors.end.is_none();
        let start_parsed = NaiveDate::parse_from_str(&params.start, DATE_FMT).ok();
        let end_parsed = NaiveDate::parse_from_str(&params.end, DATE_FMT).ok();
        params.errors.cross_field = match (both_fields_valid, start_parsed, end_parsed) {
            (true, Some(sd), Some(ed)) if sd > ed => Some("start must be on or before end".into()),
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
        // #19: strip surrounding whitespace before commit. NaiveDate / i64
        // parsers reject leading/trailing spaces, so "  2024-01-01  " would
        // otherwise fail validation; whitespace-only input collapses to "" and
        // hits the existing "must not be empty" error.
        let trimmed = new_text.trim();
        match editor.field {
            ScenarioStartupField::Start => {
                commit_w.send(ScenarioStartupParamCommit::Start(trimmed.to_string()));
                params.dirty = true;
            }
            ScenarioStartupField::End => {
                commit_w.send(ScenarioStartupParamCommit::End(trimmed.to_string()));
                params.dirty = true;
            }
            ScenarioStartupField::InitialCash => {
                commit_w.send(ScenarioStartupParamCommit::InitialCash(trimmed.to_string()));
                params.dirty = true;
            }
            ScenarioStartupField::Granularity | ScenarioStartupField::CrossField => {}
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
    mut q: Query<
        (Entity, Option<&ReadOnly>, &mut CosmicBackgroundColor),
        With<ScenarioStartupFieldEditor>,
    >,
) {
    let disabled = is_panel_disabled(&progress, &paths);
    let target_bg = if disabled {
        FIELD_BG_DISABLED
    } else {
        FIELD_BG_ACTIVE
    };
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
/// Writes through `Sprite`/`Text2d` are guarded with `!= new` to avoid
/// firing `Changed<T>` every frame.
pub fn update_scenario_startup_param_ui_system(
    params: Res<ScenarioStartupParams>,
    progress: Res<ReplayStartupProgress>,
    paths: Res<ScenarioWritebackPaths>,
    mut daily_sprite_q: Query<
        &mut Sprite,
        (
            With<ScenarioStartupGranularityDailyButton>,
            Without<ScenarioStartupGranularityMinuteButton>,
        ),
    >,
    mut minute_sprite_q: Query<
        &mut Sprite,
        (
            With<ScenarioStartupGranularityMinuteButton>,
            Without<ScenarioStartupGranularityDailyButton>,
        ),
    >,
    mut label2d_q: Query<(&ScenarioStartupErrorLabel, &mut Text2d)>,
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

    for mut sprite in daily_sprite_q.iter_mut() {
        if sprite.color != daily_color {
            sprite.color = daily_color;
        }
    }
    for mut sprite in minute_sprite_q.iter_mut() {
        if sprite.color != minute_color {
            sprite.color = minute_color;
        }
    }

    for (label, mut text) in label2d_q.iter_mut() {
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

/// Replay 以外のモードでは "Startup" パネル全体を非表示にする。
pub fn apply_startup_panel_visibility_system(
    exec_mode: Res<crate::trading::ExecutionModeRes>,
    mut panel_q: Query<&mut Visibility, With<ScenarioStartupPanelRoot>>,
) {
    if !exec_mode.is_changed() {
        return;
    }
    let target = if matches!(exec_mode.mode, crate::trading::ExecutionMode::Replay) {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut vis in &mut panel_q {
        if *vis != target {
            *vis = target;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::window::{PrimaryWindow, WindowResolution};
    use bevy_cosmic_edit::prelude::CosmicFontSystem;
    use bevy_cosmic_edit::cosmic_text::FontSystem;
    use crate::ui::render_scale::update_cosmic_render_scale_system;

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
            .insert_resource(ScenarioWritebackPaths {
                cache_sidecar: None,
            })
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
        assert!(
            msg.contains("Tick"),
            "error msg should mention 'Tick': {}",
            msg
        );
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

    /// #20: sidecar (metadata) の各項目が None の初回は、Start = 3ヶ月前 /
    /// End = 今日 / initial_cash = "1000000" をデフォルトで params に入れる。
    /// granularity は対象外。sidecar に値があれば従来どおりその値を優先する
    /// （ここでは None ケースだけを検証する）。
    #[test]
    fn test_20_sync_fills_defaults_when_metadata_none() {
        let mut app = make_app();
        // metadata はすべて None のまま（init_resource の既定）。
        // ただし sync は `metadata.is_changed()` のときだけ走るため、
        // 何らかの mutate で change tick を立てる必要がある。
        // 値は None のまま、granularity に None を再代入して is_changed を発火させる。
        {
            let mut meta = app.world_mut().resource_mut::<ScenarioMetadata>();
            meta.granularity = None;
        }
        app.update();

        // 期待値はテストと同じ「今日」から default_date_range で算出して比較
        // （実行日依存を避けるための安定化）。
        let (exp_start, exp_end) = default_date_range(chrono::Local::now().date_naive());

        let params = app.world().resource::<ScenarioStartupParams>();
        assert_eq!(
            params.start, exp_start,
            "first-run start must default to 3 months before today"
        );
        assert_eq!(
            params.end, exp_end,
            "first-run end must default to today"
        );
        assert_eq!(
            params.initial_cash, "1000000",
            "first-run initial_cash must default to 1000000"
        );
    }

    /// #20 受け入れ条件: sidecar 無し初回のデフォルト値が投入された状態で、
    /// granularity を選択すれば errors.any() == false となり Run が有効化される
    /// こと。sync 直後は granularity 未選択で errors.granularity が立つため、
    /// Granularity commit を 1 つ送って解消する経路までを検証する。
    #[test]
    fn test_20_defaults_enable_run_after_granularity_selected() {
        let mut app = make_app();
        // metadata 全 None。is_changed を立てるため granularity に None を再代入。
        {
            let mut meta = app.world_mut().resource_mut::<ScenarioMetadata>();
            meta.granularity = None;
        }
        app.update();

        // sync 直後: デフォルト日付/cash は入るが granularity 未選択でエラーが残る。
        {
            let params = app.world().resource::<ScenarioStartupParams>();
            assert!(
                params.errors.granularity.is_some(),
                "precondition: granularity unselected error must block Run before selection"
            );
        }

        // granularity を選択（commit）すると errors が解消し Run 有効化。
        app.world_mut()
            .send_event(ScenarioStartupParamCommit::Granularity(GranularityChoice::Daily));
        app.update();

        let params = app.world().resource::<ScenarioStartupParams>();
        assert!(
            !params.errors.any(),
            "with default dates/cash + a selected granularity, no error must remain (Run enabled)"
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
            .send_event(ScenarioStartupParamCommit::InitialCash(
                "not-a-number".into(),
            ));
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
        assert!(
            !params.writeback_pending,
            "writeback_pending must stay false"
        );
        assert!(params.errors.start.is_none(), "errors must stay untouched");
    }

    /// #19: the STARTUP input pipeline must strip whitespace before commit.
    /// Typing "  2024-01-01  " (with surrounding spaces) into the Start field
    /// must commit the trimmed "2024-01-01" — `NaiveDate::parse_from_str`
    /// rejects surrounding spaces, so without stripping this would error out.
    #[test]
    fn test_19_input_strips_whitespace_before_commit() {
        let mut app = make_app();
        app.add_event::<CosmicTextChanged>().add_systems(
            Update,
            (
                scenario_startup_param_input_system,
                commit_startup_params_to_scenario_system,
            )
                .chain(),
        );

        let editor = app
            .world_mut()
            .spawn(ScenarioStartupFieldEditor {
                field: ScenarioStartupField::Start,
            })
            .id();

        app.world_mut()
            .send_event(CosmicTextChanged((editor, "  2024-01-01  ".into())));
        app.update();

        let params = app.world().resource::<ScenarioStartupParams>();
        assert_eq!(
            params.start, "2024-01-01",
            "surrounding whitespace must be stripped before commit/validation"
        );
        assert!(
            params.errors.start.is_none(),
            "trimmed valid date must pass validation"
        );
    }

    /// #19: the whitespace-strip pipeline must also cover the InitialCash field.
    /// InitialCash commits through the `i64::parse` path (separate from the date
    /// fields), and `"  1000  ".parse::<i64>()` is an Err without stripping — so
    /// "Start で検証済み＝全フィールド OK" does not hold. Typing "  1000  " must
    /// commit the trimmed "1000" and leave no error.
    #[test]
    fn test_19_initial_cash_input_strips_whitespace_before_commit() {
        let mut app = make_app();
        app.add_event::<CosmicTextChanged>().add_systems(
            Update,
            (
                scenario_startup_param_input_system,
                commit_startup_params_to_scenario_system,
            )
                .chain(),
        );

        let editor = app
            .world_mut()
            .spawn(ScenarioStartupFieldEditor {
                field: ScenarioStartupField::InitialCash,
            })
            .id();

        app.world_mut()
            .send_event(CosmicTextChanged((editor, "  1000  ".into())));
        app.update();

        let params = app.world().resource::<ScenarioStartupParams>();
        assert_eq!(
            params.initial_cash, "1000",
            "surrounding whitespace must be stripped before the i64 parse/commit"
        );
        assert!(
            params.errors.initial_cash.is_none(),
            "trimmed valid integer must pass validation"
        );
    }

    /// #19 受け入れ条件: 空白のみの入力は trim 後に空文字へ潰れ、既存の
    /// "must not be empty" バリデーションに当たること。Start フィールドへ
    /// "   " を入力 → commit は "" → `validate_date_field("start", "")` が
    /// Err を返し、metadata は変更されず writeback も走らない。
    #[test]
    fn test_19_whitespace_only_input_collapses_to_empty_error() {
        let mut app = make_app();
        app.add_event::<CosmicTextChanged>().add_systems(
            Update,
            (
                scenario_startup_param_input_system,
                commit_startup_params_to_scenario_system,
            )
                .chain(),
        );

        let editor = app
            .world_mut()
            .spawn(ScenarioStartupFieldEditor {
                field: ScenarioStartupField::Start,
            })
            .id();

        app.world_mut()
            .send_event(CosmicTextChanged((editor, "   ".into())));
        app.update();

        let params = app.world().resource::<ScenarioStartupParams>();
        let meta = app.world().resource::<ScenarioMetadata>();
        assert_eq!(
            params.errors.start.as_deref(),
            Some("start must not be empty"),
            "whitespace-only input must collapse to empty and hit the must-not-be-empty error"
        );
        assert!(meta.start.is_none(), "metadata must not be mutated on empty input");
        assert!(
            !params.writeback_pending,
            "invalid (empty) input must not schedule a writeback"
        );
    }

    /// #20 review (Medium) 受け入れ条件: 空白のみの InitialCash 入力は trim 後に
    /// 空文字へ潰れ、`s.parse::<i64>()` の "invalid integer" ではなく
    /// "initial cash must not be empty" バリデーションに当たること。
    /// metadata は変更されず writeback も走らない。
    #[test]
    fn test_19_initial_cash_whitespace_only_collapses_to_empty_error() {
        let mut app = make_app();
        app.add_event::<CosmicTextChanged>().add_systems(
            Update,
            (
                scenario_startup_param_input_system,
                commit_startup_params_to_scenario_system,
            )
                .chain(),
        );

        let editor = app
            .world_mut()
            .spawn(ScenarioStartupFieldEditor {
                field: ScenarioStartupField::InitialCash,
            })
            .id();

        app.world_mut()
            .send_event(CosmicTextChanged((editor, "   ".into())));
        app.update();

        let params = app.world().resource::<ScenarioStartupParams>();
        let meta = app.world().resource::<ScenarioMetadata>();
        assert_eq!(
            params.errors.initial_cash.as_deref(),
            Some("initial cash must not be empty"),
            "whitespace-only initial cash must collapse to empty and hit the must-not-be-empty error, not 'invalid integer'"
        );
        assert!(
            meta.initial_cash.is_none(),
            "metadata must not be mutated on empty input"
        );
        assert!(
            !params.writeback_pending,
            "invalid (empty) input must not schedule a writeback"
        );
    }

    /// #20: sidecar に値が無い初回のデフォルト日付計算。`default_date_range(today)`
    /// は (Start=today−3ヶ月, End=today) を `%Y-%m-%d` で返す純関数。月末跨ぎは
    /// chrono の `checked_sub_months` 仕様どおり対象月の末日へクランプされること。
    #[test]
    fn test_20_default_date_range_clamps_month_end() {
        // 通常ケース: 2024-08-15 − 3ヶ月 = 2024-05-15、End は当日。
        let (start, end) = default_date_range(NaiveDate::from_ymd_opt(2024, 8, 15).unwrap());
        assert_eq!(start, "2024-05-15", "start must be exactly 3 months before today");
        assert_eq!(end, "2024-08-15", "end must be today");

        // 月末跨ぎ: 2024-05-31 − 3ヶ月 = 2024-02 の末日 (閏年なので 02-29) へクランプ。
        let (start, end) = default_date_range(NaiveDate::from_ymd_opt(2024, 5, 31).unwrap());
        assert_eq!(start, "2024-02-29", "month-end must clamp to the target month's last valid day");
        assert_eq!(end, "2024-05-31", "end must be today");

        // 年跨ぎ: 2024-01-31 − 3ヶ月 = 2023-10-31。
        let (start, end) = default_date_range(NaiveDate::from_ymd_opt(2024, 1, 31).unwrap());
        assert_eq!(start, "2023-10-31", "subtraction must roll back across the year boundary");
        assert_eq!(end, "2024-01-31", "end must be today");
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
        assert!(
            app.world()
                .resource::<ScenarioStartupParams>()
                .errors
                .cross_field
                .is_some()
        );

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
        assert_eq!(
            v.get("layout").unwrap().get("foo").unwrap().as_str(),
            Some("bar")
        );
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

    use crate::trading::{ExecutionMode, ExecutionModeRes};

    fn make_visibility_app() -> App {
        let mut app = App::new();
        app.init_resource::<ExecutionModeRes>();
        app.add_systems(Update, apply_startup_panel_visibility_system);
        app
    }

    fn spawn_panel(app: &mut App) -> Entity {
        app.world_mut()
            .spawn((Visibility::Inherited, ScenarioStartupPanelRoot))
            .id()
    }

    #[test]
    fn startup_panel_visible_in_replay() {
        let mut app = make_visibility_app();
        let e = spawn_panel(&mut app);
        app.update();
        assert_eq!(
            *app.world().entity(e).get::<Visibility>().unwrap(),
            Visibility::Inherited,
        );
    }

    #[test]
    fn startup_panel_hidden_in_live_manual() {
        let mut app = make_visibility_app();
        let e = app
            .world_mut()
            .spawn((Visibility::Inherited, ScenarioStartupPanelRoot))
            .id();
        app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveManual;
        app.update();
        assert_eq!(
            *app.world().entity(e).get::<Visibility>().unwrap(),
            Visibility::Hidden,
        );
    }

    #[test]
    fn startup_panel_hidden_in_live_auto() {
        let mut app = make_visibility_app();
        let e = spawn_panel(&mut app);
        app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveAuto;
        app.update();
        assert_eq!(
            *app.world().entity(e).get::<Visibility>().unwrap(),
            Visibility::Hidden,
        );
    }

    #[test]
    fn mode_switch_toggles_panel_display() {
        let mut app = make_visibility_app();
        let e = spawn_panel(&mut app);

        app.update();
        assert_eq!(
            *app.world().entity(e).get::<Visibility>().unwrap(),
            Visibility::Inherited,
        );

        app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveManual;
        app.update();
        assert_eq!(
            *app.world().entity(e).get::<Visibility>().unwrap(),
            Visibility::Hidden,
        );

        app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::Replay;
        app.update();
        assert_eq!(
            *app.world().entity(e).get::<Visibility>().unwrap(),
            Visibility::Inherited,
        );
    }

    /// Startup-field DPI regression: when the primary window reports
    /// scale_factor 2.0, the field editor's CosmicRenderScale must be driven
    /// up to >= 2.0 so the cosmic sprite is supersampled to match DPI.
    /// Goes through the REAL spawn path (`spawn_scenario_startup_input_fields`)
    /// so it stays RED until `spawn_field` adopts `RenderScaleResponsive`.
    #[test]
    fn startup_field_render_scale_follows_window_dpi() {
        let mut app = App::new();
        app.insert_resource(CosmicFontSystem(FontSystem::new()))
            .add_systems(
                Update,
                (
                    spawn_scenario_startup_input_fields,
                    update_cosmic_render_scale_system,
                )
                    .chain(),
            );

        // Primary window at 2x DPI; no Camera2d (system's camera_q errs -> zoom 1.0).
        app.world_mut().spawn((
            Window {
                resolution: WindowResolution::new(1280.0, 720.0)
                    .with_scale_factor_override(2.0),
                ..default()
            },
            PrimaryWindow,
        ));

        // Host that the real spawn system attaches a field editor to.
        app.world_mut().spawn(ScenarioStartupStartFieldHost);

        // First update spawns the field editor; second lets the render-scale
        // system observe it (spawn commands apply at end of frame).
        app.update();
        app.update();

        let mut q = app
            .world_mut()
            .query::<(&ScenarioStartupFieldEditor, &CosmicRenderScale)>();
        let (_, scale) = q
            .iter(app.world())
            .next()
            .expect("spawn_scenario_startup_input_fields should create a field editor");
        assert!(
            scale.0 >= 1.99,
            "field CosmicRenderScale should follow 2x DPI, got {}",
            scale.0
        );
    }

    /// Tint regression: a `bevy_cosmic_edit` field renders light glyphs onto a
    /// dark `CosmicBackgroundColor` texture, and the host `Sprite.color` is a
    /// MULTIPLY tint over that texture. The dark color must live ONLY in
    /// `CosmicBackgroundColor`; the `Sprite.color` must be `WHITE` (×1) or the
    /// light glyphs collapse to near-black and the value is unreadable.
    /// Drives the real spawn path so it stays honest about `spawn_field`.
    #[test]
    fn startup_field_sprite_tint_is_white_with_dark_cosmic_bg() {
        let mut app = App::new();
        app.insert_resource(CosmicFontSystem(FontSystem::new()))
            .add_systems(Update, spawn_scenario_startup_input_fields);

        // Host that the real spawn system attaches a field editor to.
        app.world_mut().spawn(ScenarioStartupStartFieldHost);

        // First update spawns the field editor; second lets the spawn commands
        // apply (commands flush at end of frame).
        app.update();
        app.update();

        let mut q = app
            .world_mut()
            .query_filtered::<(&Sprite, &CosmicBackgroundColor), With<ScenarioStartupFieldEditor>>(
            );
        let (sprite, bg) = q
            .iter(app.world())
            .next()
            .expect("spawn_scenario_startup_input_fields should create a field editor");

        assert_eq!(
            sprite.color,
            Color::WHITE,
            "Sprite tint must be WHITE so the cosmic texture (light glyphs) shows true; \
             a dark tint multiplies the glyphs to near-black"
        );
        assert_eq!(
            bg.0, FIELD_BG_ACTIVE,
            "the dark field background must come from CosmicBackgroundColor, not the Sprite tint"
        );
        assert_eq!(
            sprite.custom_size,
            Some(Vec2::new(120.0, 22.0)),
            "field height must be 22.0 (the 32.0 bump overlapped neighbouring rows)"
        );
    }
}
