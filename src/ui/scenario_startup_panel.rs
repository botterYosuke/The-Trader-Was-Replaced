use bevy::prelude::*;
use chrono::{Months, NaiveDate};

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};

use crate::replay::startup_progress::ReplayStartupProgress;


use crate::ui::components::{
    GranularityChoice, PanelKind, ScenarioMetadata, ScenarioStartupCashFieldHost,
    ScenarioStartupEndFieldHost, ScenarioStartupErrorLabel, ScenarioStartupField,
    ScenarioStartupFieldEditor, ScenarioStartupFieldText, ScenarioStartupFocus,
    ScenarioStartupGranularityDailyButton, ScenarioStartupGranularityMinuteButton,
    ScenarioStartupPanelRoot, ScenarioStartupParams, ScenarioStartupStartFieldHost,
    ScenarioWritebackPaths, atomic_mutate_scenario_object,
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

#[derive(Message, Debug, Clone)]
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
                bevy::sprite::Anchor::CENTER_RIGHT,
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
                move |_trigger: On<Pointer<Click>>,
                      mut commit_w: MessageWriter<ScenarioStartupParamCommit>,
                      mut params: ResMut<ScenarioStartupParams>,
                      progress: Res<ReplayStartupProgress>,
                      paths: Res<ScenarioWritebackPaths>| {
                    if is_panel_disabled(&progress, &paths) {
                        return;
                    }
                    commit_w.write(ScenarioStartupParamCommit::Granularity(choice));
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
            bevy::sprite::Anchor::CENTER_RIGHT,
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
        host: Entity,
        field: ScenarioStartupField,
    ) {
        // Slice 6a (#50): cosmic-edit から Bevy 2D primitive へ。
        // - Sprite: 入力欄の背景矩形 (色は readonly_system が後から塗り替える)
        // - Text2d: 値の表示 (`scenario_startup_field_render_system` が
        //   `ScenarioStartupFieldText.0` を見て差分更新する。本 commit には未実装)
        // - `ScenarioStartupFieldEditor` / `ScenarioStartupFieldText` を同一 entity に持たせ、
        //   input_system は Editor で focus 対象を識別し、Text で本文を書き換える。
        let entity = commands
            .spawn((
                Sprite {
                    custom_size: Some(Vec2::new(120.0, 22.0)),
                    color: FIELD_BG_ACTIVE,
                    ..default()
                },
                Transform::from_xyz(0.0, 0.0, 0.1),
                ScenarioStartupFieldEditor { field },
                ScenarioStartupFieldText::default(),
            ))
            .observe(
                move |_trigger: On<Pointer<Click>>,
                      mut focus: ResMut<ScenarioStartupFocus>| {
                    focus.field = Some(field);
                },
            )
            .with_children(|parent| {
                parent.spawn((
                    Text2d::new(""),
                    TextFont {
                        font_size: 9.0,
                        ..default()
                    },
                    TextColor(Color::srgb_u8(220, 220, 220)),
                    // Sprite の中央(0,0)に置く。Text2d は origin 基準で描画されるので、
                    // 左寄せにしたい場合は `JustifyText::Left` + Anchor 調整が必要だが、
                    // 旧 cosmic も TopLeft padding=1 だっただけで強い揃え保証は無いため、
                    // 中央配置で許容する (視認性の最終確認は手動 E2E)。
                    Transform::from_xyz(0.0, 0.0, 0.1),
                ));
            })
            .id();
        commands.entity(host).add_child(entity);
    }

    if let Ok(host) = start_host_q.single() {
        spawn_field(
            &mut commands,
            host,
            ScenarioStartupField::Start,
        );
    }
    if let Ok(host) = end_host_q.single() {
        spawn_field(
            &mut commands,
            host,
            ScenarioStartupField::End,
        );
    }
    if let Ok(host) = cash_host_q.single() {
        spawn_field(
            &mut commands,
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
    mut events: MessageReader<ScenarioStartupParamCommit>,
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
    mut editors_q: Query<(&ScenarioStartupFieldEditor, &mut ScenarioStartupFieldText)>,
) {
    if !params.is_changed() {
        return;
    }

    for (editor, mut text) in editors_q.iter_mut() {
        let expected: &str = match editor.field {
            ScenarioStartupField::Start => &params.start,
            ScenarioStartupField::End => &params.end,
            ScenarioStartupField::InitialCash => &params.initial_cash,
            ScenarioStartupField::Granularity | ScenarioStartupField::CrossField => continue,
        };
        // Slice 6a (#50): 規約2 — 値が同じなら触らない。
        // `DerefMut` を経由しないことで `Changed<ScenarioStartupFieldText>` を立てず、
        // 後段 render system の不要な再走を避ける。
        if text.0 != expected {
            text.0 = expected.to_string();
        }
    }
}

/// Slice 6a (#50): parent の `ScenarioStartupFieldText.0` を子 `Text2d` に転記する。
///
/// `Changed<ScenarioStartupFieldText>` で発火し、差分書き込み（規約2）で
/// `text2d.0 != parent_text.0` のときだけ touch する。子 Text2d は
/// `spawn_field` が `parent.spawn((Text2d, ...))` で 1 個だけ生やす想定。
/// 0 件 / 複数は warn のみ（spawn 経路の bug の早期発見）。
pub fn scenario_startup_field_render_system(
    parents_q: Query<(Entity, &ScenarioStartupFieldText, &Children), Changed<ScenarioStartupFieldText>>,
    mut text2d_q: Query<&mut Text2d>,
) {
    for (parent, field_text, children) in parents_q.iter() {
        let mut hits = 0usize;
        for child in children.iter() {
            if let Ok(mut t) = text2d_q.get_mut(child) {
                hits += 1;
                if t.0 != field_text.0 {
                    t.0 = field_text.0.clone();
                }
            }
        }
        if hits == 0 {
            warn!(
                "scenario_startup_field_render_system: parent {:?} has no Text2d child (spawn path regression?)",
                parent
            );
        } else if hits > 1 {
            warn!(
                "scenario_startup_field_render_system: parent {:?} has {} Text2d children (expected 1)",
                parent, hits
            );
        }
    }
}

/// Slice 6a (#50): Startup 入力 system。`Messages<KeyboardInput>::drain` +
/// `ScenarioStartupFocus` を読む自前ドライブ。
///
/// - focus None / panel disabled のときは drain して早期 return（後段 cosmic / menu_bar
///   への二重配送を防ぐ instrument_picker / find_field_input_system と同じ流派）。
/// - Backspace / Character は focus 中 field の `ScenarioStartupFieldText.0` に
///   即時反映。`Changed<ScenarioStartupFieldText>` を `scenario_startup_field_render_system`
///   が拾って子 Text2d に転記する。
/// - Enter で `ScenarioStartupParamCommit::{Start,End,InitialCash}` を emit
///   （Granularity / CrossField は input 対象外でスキップ）。Enter 後も focus は
///   維持する（find と対称、連続コミット可能）。
/// - Tab は focus を Start → End → InitialCash → Start で循環し、`kb_events.clear()`
///   で残イベントを破棄して後段への二重配送を防ぐ（instrument_picker §D-3-b と同パターン）。
/// - Esc は focus を None に落とす（panel は閉じない／find と異なる：Startup window は
///   常時表示、focus 解除のみが目的）。
/// - 文字 filter: InitialCash は `is_ascii_digit()` のみ、Start/End は
///   `is_ascii_digit() || c == '-'` のみ通す。それ以外の char は drop。
pub fn scenario_startup_param_input_system(
    mut kb_events: ResMut<Messages<KeyboardInput>>,
    mut focus: ResMut<ScenarioStartupFocus>,
    mut text_q: Query<(&ScenarioStartupFieldEditor, &mut ScenarioStartupFieldText)>,
    mut commit_w: MessageWriter<ScenarioStartupParamCommit>,
    mut params: ResMut<ScenarioStartupParams>,
    progress: Res<ReplayStartupProgress>,
    paths: Res<ScenarioWritebackPaths>,
) {
    // Panel disabled: drain して捨てる（focus は維持。再 enable 時に同じ field から再開できる）。
    if is_panel_disabled(&progress, &paths) {
        let _ = kb_events.drain().count();
        return;
    }

    // Focus 無し: drain せず素通り（menu_bar / picker などが後段で読めるよう残す）。
    let Some(focused_field) = focus.field else {
        return;
    };

    for ev in kb_events.drain() {
        if ev.state != ButtonState::Pressed {
            continue;
        }
        match &ev.logical_key {
            Key::Backspace => {
                for (editor, mut text) in text_q.iter_mut() {
                    if editor.field == focused_field {
                        text.0.pop();
                        break;
                    }
                }
            }
            Key::Tab => {
                // Tab 循環: 残イベントは drain ループ自体が消費するので
                // kb_events.clear() は呼ばない（E0499 回避、find_field_input_system / picker_searchbox_input_system
                // と同パターン）。
                focus.field = Some(match focused_field {
                    ScenarioStartupField::Start => ScenarioStartupField::End,
                    ScenarioStartupField::End => ScenarioStartupField::InitialCash,
                    ScenarioStartupField::InitialCash => ScenarioStartupField::Start,
                    ScenarioStartupField::Granularity | ScenarioStartupField::CrossField => {
                        ScenarioStartupField::Start
                    }
                });
            }
            Key::Escape => {
                focus.field = None;
                return;
            }
            Key::Enter => {
                // 現在 field の text を読んで commit。focus は維持（連続コミット可）。
                let mut value: Option<String> = None;
                for (editor, text) in text_q.iter() {
                    if editor.field == focused_field {
                        value = Some(text.0.clone());
                        break;
                    }
                }
                if let Some(v) = value {
                    match focused_field {
                        ScenarioStartupField::Start => {
                            commit_w.write(ScenarioStartupParamCommit::Start(v));
                            params.dirty = true;
                        }
                        ScenarioStartupField::End => {
                            commit_w.write(ScenarioStartupParamCommit::End(v));
                            params.dirty = true;
                        }
                        ScenarioStartupField::InitialCash => {
                            commit_w.write(ScenarioStartupParamCommit::InitialCash(v));
                            params.dirty = true;
                        }
                        ScenarioStartupField::Granularity
                        | ScenarioStartupField::CrossField => {}
                    }
                }
            }
            Key::Character(s) => {
                for ch in s.chars() {
                    let pass = match focused_field {
                        ScenarioStartupField::InitialCash => ch.is_ascii_digit(),
                        ScenarioStartupField::Start | ScenarioStartupField::End => {
                            ch.is_ascii_digit() || ch == '-'
                        }
                        ScenarioStartupField::Granularity
                        | ScenarioStartupField::CrossField => false,
                    };
                    if !pass {
                        continue;
                    }
                    for (editor, mut text) in text_q.iter_mut() {
                        if editor.field == focused_field {
                            text.0.push(ch);
                            break;
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Dim the field background on the 3 startup field editor entities while
/// `is_panel_disabled`. Post cosmic 撤去では Sprite.color を直接塗ることで
/// visual disable を表現する（input gate は input_system 側の
/// `is_panel_disabled` short-circuit で完結）。
pub fn enforce_scenario_startup_panel_readonly_system(
    progress: Res<ReplayStartupProgress>,
    paths: Res<ScenarioWritebackPaths>,
    mut q: Query<&mut Sprite, With<ScenarioStartupFieldEditor>>,
) {
    let target = if is_panel_disabled(&progress, &paths) {
        FIELD_BG_DISABLED
    } else {
        FIELD_BG_ACTIVE
    };
    for mut sprite in q.iter_mut() {
        if sprite.color != target {
            sprite.color = target;
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

    /// Finding A (E2E §13 regression): the visible-flag short-circuit on the
    /// commit/input/writeback systems is necessary but not sufficient — without
    /// also dimming the editors via `Sprite.color` the user has no visual cue
    /// that the panel is disabled during a Run, and `FIELD_BG_ACTIVE` would
    /// leak through after auto-hide flips `progress.visible`.
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
                Sprite {
                    color: FIELD_BG_ACTIVE,
                    ..default()
                },
            ))
            .id();

        // Run while progress hidden — editor must be editable.
        app.update();
        assert_eq!(
            app.world().get::<Sprite>(e).unwrap().color,
            FIELD_BG_ACTIVE
        );

        // Flip to visible — editor must be dimmed.
        app.world_mut()
            .resource_mut::<ReplayStartupProgress>()
            .visible = true;
        app.update();
        assert_eq!(
            app.world().get::<Sprite>(e).unwrap().color,
            FIELD_BG_DISABLED
        );

        // Flip back — bg restored.
        app.world_mut()
            .resource_mut::<ReplayStartupProgress>()
            .visible = false;
        app.update();
        assert_eq!(
            app.world().get::<Sprite>(e).unwrap().color,
            FIELD_BG_ACTIVE
        );
    }

    /// Cache-unavailable (`ScenarioWritebackPaths.cache_sidecar = None`) also
    /// disables the panel per plan §"cache_sidecar == None" — the dimming
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
                Sprite {
                    color: FIELD_BG_ACTIVE,
                    ..default()
                },
            ))
            .id();

        app.update();
        assert_eq!(
            app.world().get::<Sprite>(e).unwrap().color,
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
            .add_message::<ScenarioStartupParamCommit>()
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
            .write_message(ScenarioStartupParamCommit::Granularity(GranularityChoice::Daily));
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
            .write_message(ScenarioStartupParamCommit::Start("not-a-date".into()));
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
            .write_message(ScenarioStartupParamCommit::Start("bad".into()));
        app.world_mut()
            .write_message(ScenarioStartupParamCommit::End("also-bad".into()));
        app.update();
        {
            let params = app.world().resource::<ScenarioStartupParams>();
            assert!(params.errors.start.is_some());
            assert!(params.errors.end.is_some());
        }

        app.world_mut()
            .write_message(ScenarioStartupParamCommit::Start("2024-01-01".into()));
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
            .write_message(ScenarioStartupParamCommit::Start("2024-01-01".into()));
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
            .write_message(ScenarioStartupParamCommit::Start("2024-06-01".into()));
        app.world_mut()
            .write_message(ScenarioStartupParamCommit::End("2024-01-01".into()));
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
            .write_message(ScenarioStartupParamCommit::Start("2024-01-01".into()));
        app.world_mut()
            .write_message(ScenarioStartupParamCommit::InitialCash(
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
            .write_message(ScenarioStartupParamCommit::Start("2024-01-01".into()));
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
            .write_message(ScenarioStartupParamCommit::Start("2024-06-01".into()));
        app.world_mut()
            .write_message(ScenarioStartupParamCommit::End("2024-01-01".into()));
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
            .write_message(ScenarioStartupParamCommit::End("not-a-date".into()));
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
            .add_message::<ScenarioStartupParamCommit>()
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
        app.add_message::<crate::ui::components::ScenarioLoadedFromFile>();
        app.add_message::<crate::ui::components::ScenarioClearedFromFile>();
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
            .add_message::<ScenarioStartupParamCommit>()
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

}
