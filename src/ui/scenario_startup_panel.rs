use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy_ui_text_input::actions::{TextInputAction, TextInputEdit};
use bevy_ui_text_input::{TextInputContents, TextInputFilter, TextInputMode, TextInputNode, TextInputQueue};
use chrono::{Months, NaiveDate};

use crate::replay::startup_progress::ReplayStartupProgress;
use crate::ui::screen_window::{ScreenWindowSpec, spawn_screen_window};

/// `TextInputBuffer` の全文を `src` で置き換える action 列を queue に積む（SelectAll→Paste）。
fn queue_full_text(queue: &mut TextInputQueue, src: &str) {
    queue.add(TextInputAction::Edit(TextInputEdit::SelectAll));
    queue.add(TextInputAction::Edit(TextInputEdit::Paste(src.to_string())));
}

use crate::ui::components::{
    GranularityChoice, PanelKind, ScenarioMetadata, ScenarioStartupCashFieldHost,
    ScenarioStartupEndFieldHost, ScenarioStartupErrorLabel, ScenarioStartupField,
    ScenarioStartupFieldEditor, ScenarioStartupGranularityDailyButton,
    ScenarioStartupGranularityMinuteButton, ScenarioStartupPanelRoot, ScenarioStartupParams,
    ScenarioStartupStartFieldHost, ScenarioWritebackPaths,
    atomic_mutate_scenario_object,
};
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

/// screen-space (Bevy UI Node) STARTUP window（ADR 0003）。host marker / error-label
/// marker / granularity-button marker を既存システムが期待する数・種類で再現する。
/// 不変条件: × クローズボタン無し（`closeable: false`）・Replay 限定表示。
/// Startup 時に startup window を一度だけ spawn する system ラッパ。
/// 本体は helper `spawn_scenario_startup_window(&mut Commands)`（4d の dispatcher 復元 arm も同 helper を呼ぶ）。
pub fn spawn_scenario_startup_window_system(mut commands: Commands) {
    spawn_scenario_startup_window(&mut commands);
}

const STARTUP_LABEL_COLOR: Color = Color::srgb(0.78, 0.82, 0.92);
const GRAN_BTN_BG: Color = Color::srgba(0.10, 0.10, 0.16, 1.0);
/// フィールド host Node のサイズ（編集領域）。
const FIELD_W: f32 = 130.0;
const FIELD_H: f32 = 22.0;
/// ラベル列の幅（右寄せでフィールドとの間隔を一定に保つ）。
const LABEL_W: f32 = 90.0;

pub fn spawn_scenario_startup_window(commands: &mut Commands) {
    const WINDOW_SIZE: Vec2 = Vec2::new(300.0, 250.0);
    const WINDOW_POSITION: Vec2 = Vec2::new(60.0, 500.0);
    const ACCENT: Color = Color::srgba(0.5, 0.7, 1.0, 0.4);

    let (root, content_area, _title_bar) = spawn_screen_window(
        commands,
        ScreenWindowSpec {
            title: "STARTUP".to_string(),
            size: WINDOW_SIZE,
            position: WINDOW_POSITION,
            accent: ACCENT,
            // Startup の不変条件: × クローズボタンは出さない。
            closeable: false,
        },
    );
    commands
        .entity(root)
        .insert((PanelKind::Startup, ScenarioStartupPanelRoot));
    // content_area に内側 padding を足す（行が枠に張り付かないように）。
    // spawn_screen_window が付けた Node を上書きして padding/row_gap を加える。
    commands.entity(content_area).insert(Node {
        width: Val::Percent(100.0),
        flex_grow: 1.0,
        flex_direction: FlexDirection::Column,
        padding: UiRect::all(Val::Px(8.0)),
        row_gap: Val::Px(2.0),
        ..default()
    });

    // ── ラベル + フィールド host を 1 行 spawn する helper（Row Node）──
    fn spawn_field_row(
        commands: &mut Commands,
        parent: Entity,
        label: &str,
        host_marker: impl Bundle,
    ) {
        let row = commands
            .spawn(Node {
                width: Val::Percent(100.0),
                height: Val::Px(FIELD_H),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(10.0),
                ..default()
            })
            .id();
        commands.entity(parent).add_child(row);

        let lbl = commands
            .spawn((
                Node {
                    width: Val::Px(LABEL_W),
                    justify_content: JustifyContent::FlexEnd,
                    ..default()
                },
                Text::new(label),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(STARTUP_LABEL_COLOR),
                TextLayout::new_with_justify(JustifyText::Right),
            ))
            .id();
        commands.entity(row).add_child(lbl);

        let host = commands
            .spawn((
                Node {
                    width: Val::Px(FIELD_W),
                    height: Val::Px(FIELD_H),
                    ..default()
                },
                host_marker,
            ))
            .id();
        commands.entity(row).add_child(host);
    }

    // ── エラーラベル (空文字で spawn、update system が後で書く) helper ──
    fn spawn_error_label(commands: &mut Commands, parent: Entity, field: ScenarioStartupField) {
        let err = commands
            .spawn((
                Text::new(""),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(ERROR_COLOR),
                ScenarioStartupErrorLabel { field },
            ))
            .id();
        commands.entity(parent).add_child(err);
    }

    // ── granularity ボタン (Node + Button + marker、子に Text ラベル) helper ──
    fn spawn_granularity_btn(
        commands: &mut Commands,
        parent: Entity,
        label: &str,
        marker: impl Bundle,
        choice: GranularityChoice,
    ) {
        let btn = commands
            .spawn((
                Node {
                    width: Val::Px(54.0),
                    height: Val::Px(18.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                BackgroundColor(GRAN_BTN_BG),
                Button,
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
                    commit_w.write(ScenarioStartupParamCommit::Granularity(choice));
                    params.dirty = true;
                },
            )
            .id();
        commands.entity(parent).add_child(btn);

        let txt = commands
            .spawn((
                Text::new(label),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(STARTUP_LABEL_COLOR),
            ))
            .id();
        commands.entity(btn).add_child(txt);
    }

    // (a) Start 行 + (b) Start エラー
    spawn_field_row(commands, content_area, "Start", ScenarioStartupStartFieldHost);
    spawn_error_label(commands, content_area, ScenarioStartupField::Start);

    // (c) End 行 + End エラー
    spawn_field_row(commands, content_area, "End", ScenarioStartupEndFieldHost);
    spawn_error_label(commands, content_area, ScenarioStartupField::End);

    // (d) Granularity ラベル + 2 ボタン + Granularity エラー
    let gran_row = commands
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Px(FIELD_H),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.0),
            ..default()
        })
        .id();
    commands.entity(content_area).add_child(gran_row);
    let gran_label = commands
        .spawn((
            Node {
                width: Val::Px(LABEL_W),
                justify_content: JustifyContent::FlexEnd,
                ..default()
            },
            Text::new("Granularity"),
            TextFont {
                font_size: 11.0,
                ..default()
            },
            TextColor(STARTUP_LABEL_COLOR),
            TextLayout::new_with_justify(JustifyText::Right),
        ))
        .id();
    commands.entity(gran_row).add_child(gran_label);
    spawn_granularity_btn(
        commands,
        gran_row,
        "Daily",
        ScenarioStartupGranularityDailyButton,
        GranularityChoice::Daily,
    );
    spawn_granularity_btn(
        commands,
        gran_row,
        "Minute",
        ScenarioStartupGranularityMinuteButton,
        GranularityChoice::Minute,
    );
    spawn_error_label(commands, content_area, ScenarioStartupField::Granularity);

    // (e) Initial cash 行 + エラー
    spawn_field_row(commands, content_area, "Initial cash", ScenarioStartupCashFieldHost);
    spawn_error_label(commands, content_area, ScenarioStartupField::InitialCash);

    // (f) CrossField エラー
    spawn_error_label(commands, content_area, ScenarioStartupField::CrossField);
}

/// Attach a `bevy_ui_text_input` `TextInputNode` to each field host. Focus is handled
/// by the `TextInputNode` on_add observer (pointer-down). Date fields are `SingleLine`
/// (no filter); the cash field uses an `Integer` filter so only digits/sign are accepted.
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
    fn spawn_field(commands: &mut Commands, host: Entity, field: ScenarioStartupField) {
        // 初期残高は整数のみ受け付ける（Integer filter）。日付は自由入力で validate 側に任せる。
        let filter = match field {
            ScenarioStartupField::InitialCash => Some(TextInputFilter::Integer),
            _ => None,
        };
        let entity = commands
            .spawn((
                TextInputNode {
                    mode: TextInputMode::SingleLine,
                    filter,
                    // submit（Enter）で内容を消さない／フォーカスを外さない。
                    clear_on_submit: false,
                    unfocus_on_submit: false,
                    ..default()
                },
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(Color::srgb(0.86, 0.86, 0.86)),
                BackgroundColor(FIELD_BG_ACTIVE),
                // TextInputNode は TextInputContents を require しないので明示挿入（無いと
                // Changed<TextInputContents> が発火せず editor→params 同期が死ぬ）。
                TextInputContents::default(),
                ScenarioStartupFieldEditor { field },
            ))
            .id();
        commands.entity(host).add_child(entity);
    }

    if let Ok(host) = start_host_q.single() {
        spawn_field(&mut commands, host, ScenarioStartupField::Start);
    }
    if let Ok(host) = end_host_q.single() {
        spawn_field(&mut commands, host, ScenarioStartupField::End);
    }
    if let Ok(host) = cash_host_q.single() {
        spawn_field(&mut commands, host, ScenarioStartupField::InitialCash);
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

    // diff-write: 同値の再代入で ResMut の Changed を立てると、commit→metadata→
    // sync の毎フレーム ringing で editor が再 Paste されカーソルが飛ぶ。値が変わるときだけ書く。
    let today = chrono::Local::now().date_naive();
    let (default_start, default_end) = default_date_range(today);
    let new_start = metadata.start.clone().unwrap_or(default_start);
    let new_end = metadata.end.clone().unwrap_or(default_end);
    if params.start != new_start {
        params.start = new_start;
    }
    if params.end != new_end {
        params.end = new_end;
    }

    let (new_gran, new_gran_err) = match metadata.granularity.as_deref() {
        Some(s) => match GranularityChoice::parse_canonical(s) {
            Some(choice) => (choice, None),
            None => (
                GranularityChoice::default(),
                Some(format!(
                    "unknown granularity '{}'; please select Daily or Minute to enable Run",
                    s
                )),
            ),
        },
        None => (
            GranularityChoice::default(),
            Some("Please select a granularity to enable Run".to_string()),
        ),
    };
    if params.granularity != new_gran {
        params.granularity = new_gran;
    }
    if params.errors.granularity != new_gran_err {
        params.errors.granularity = new_gran_err;
    }

    let new_cash = match metadata.initial_cash {
        Some(n) => n.to_string(),
        None => "1000000".to_string(),
    };
    if params.initial_cash != new_cash {
        params.initial_cash = new_cash;
    }
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

/// Propagate `params.{start,end,initial_cash}` strings into each `bevy_ui_text_input`
/// buffer. Gated on `params.is_changed()` to avoid resetting the user's cursor; per-field
/// we also skip when the current contents already match (no-op Paste avoidance).
pub fn sync_startup_param_editors_text_system(
    params: Res<ScenarioStartupParams>,
    mut editors_q: Query<(
        &ScenarioStartupFieldEditor,
        &TextInputContents,
        &mut TextInputQueue,
    )>,
) {
    if !params.is_changed() {
        return;
    }

    for (editor, contents, mut queue) in editors_q.iter_mut() {
        let expected: &str = match editor.field {
            ScenarioStartupField::Start => &params.start,
            ScenarioStartupField::End => &params.end,
            ScenarioStartupField::InitialCash => &params.initial_cash,
            ScenarioStartupField::Granularity | ScenarioStartupField::CrossField => continue,
        };
        // trim 比較: ユーザーが入力途中の前後空白（commit は trim 済みで params に入る）を
        // 反射 Paste で消してカーソルを飛ばさないようにする。意味的に一致するなら触らない。
        if contents.get().trim() == expected {
            continue;
        }
        queue_full_text(&mut queue, expected);
    }
}

/// editor → params 同期。`Changed<TextInputContents>` で変更されたフィールドを拾い、
/// commit イベントを送る。`sync_startup_param_editors_text_system` の Paste で起きる
/// echo（contents == params の現在値）はユーザー入力ではないので skip し、フィードバック
/// ループ（不要な writeback / dirty 振動）を防ぐ。
/// editor 生テキスト → commit イベント（#19: 前後空白を strip してフィールド別に振り分ける）。
/// `Granularity` / `CrossField` は editor を持たないので `None`。
/// whitespace-strip の単体テストはこの純関数を直接叩く（`TextInputContents` は private field で
/// テストから構築できないため）。
fn commit_for_field(field: ScenarioStartupField, raw: &str) -> Option<ScenarioStartupParamCommit> {
    let trimmed = raw.trim().to_string();
    match field {
        ScenarioStartupField::Start => Some(ScenarioStartupParamCommit::Start(trimmed)),
        ScenarioStartupField::End => Some(ScenarioStartupParamCommit::End(trimmed)),
        ScenarioStartupField::InitialCash => Some(ScenarioStartupParamCommit::InitialCash(trimmed)),
        ScenarioStartupField::Granularity | ScenarioStartupField::CrossField => None,
    }
}

pub fn scenario_startup_param_input_system(
    editors_q: Query<(&ScenarioStartupFieldEditor, &TextInputContents), Changed<TextInputContents>>,
    mut commit_w: EventWriter<ScenarioStartupParamCommit>,
    mut params: ResMut<ScenarioStartupParams>,
    progress: Res<ReplayStartupProgress>,
    paths: Res<ScenarioWritebackPaths>,
) {
    if is_panel_disabled(&progress, &paths) {
        return;
    }

    for (editor, contents) in editors_q.iter() {
        let raw = contents.get();
        // echo ガード: params の現在値と一致するなら params→editor 同期の反射なので無視。
        let current: &str = match editor.field {
            ScenarioStartupField::Start => &params.start,
            ScenarioStartupField::End => &params.end,
            ScenarioStartupField::InitialCash => &params.initial_cash,
            ScenarioStartupField::Granularity | ScenarioStartupField::CrossField => continue,
        };
        if raw.trim() == current {
            continue;
        }
        if let Some(ev) = commit_for_field(editor.field, raw) {
            commit_w.write(ev);
            params.dirty = true;
        }
    }
}

/// Disable input + dim the field background on the 3 `TextInputNode` entities
/// while `is_panel_disabled`. `bevy_ui_text_input` has no separate `ReadOnly`
/// component; `TextInputNode.is_enabled = false` blocks focus / clicks / keystrokes.
/// The commit / writeback / input systems already short-circuit on
/// `is_panel_disabled`, but disabling the node prevents the user from mutating the
/// buffer during a Run (whose trailing change could leak through after auto-hide).
pub fn enforce_scenario_startup_panel_readonly_system(
    progress: Res<ReplayStartupProgress>,
    paths: Res<ScenarioWritebackPaths>,
    // Option: enforce 単体テストは InputFocus を持たない App で走るため tolerant に。
    mut input_focus: Option<ResMut<InputFocus>>,
    mut q: Query<
        (Entity, &mut TextInputNode, &mut BackgroundColor),
        With<ScenarioStartupFieldEditor>,
    >,
) {
    let disabled = is_panel_disabled(&progress, &paths);
    let target_bg = if disabled {
        FIELD_BG_DISABLED
    } else {
        FIELD_BG_ACTIVE
    };
    for (entity, mut node, mut bg) in q.iter_mut() {
        let want_enabled = !disabled;
        if node.is_enabled != want_enabled {
            node.is_enabled = want_enabled;
        }
        // is_enabled=false は focus 中のキーボード経路を止めない（bevy_ui_text_input の
        // on_focused_keyboard_input は is_enabled を見ない）。Run 中の buffer 改変を防ぐため
        // フォーカスを明示的に外す。
        if disabled
            && let Some(focus) = input_focus.as_mut()
            && focus.0 == Some(entity)
        {
            focus.0 = None;
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
    mut daily_btn_q: Query<
        &mut BackgroundColor,
        (
            With<ScenarioStartupGranularityDailyButton>,
            Without<ScenarioStartupGranularityMinuteButton>,
        ),
    >,
    mut minute_btn_q: Query<
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

    for mut bg in daily_btn_q.iter_mut() {
        if bg.0 != daily_color {
            bg.0 = daily_color;
        }
    }
    for mut bg in minute_btn_q.iter_mut() {
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

    /// Finding A (E2E §13 regression): the visible-flag short-circuit on the
    /// commit/input/writeback systems is necessary but not sufficient — without
    /// also disabling the editors the user can still mutate the buffer during a
    /// Run. `bevy_ui_text_input` has no `ReadOnly` component; the gate is
    /// `TextInputNode.is_enabled` + a dimmed `BackgroundColor`.
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
                TextInputNode::default(),
                BackgroundColor(FIELD_BG_ACTIVE),
            ))
            .id();

        // Run while progress hidden — editor must be enabled.
        app.update();
        assert!(app.world().get::<TextInputNode>(e).unwrap().is_enabled);
        assert_eq!(
            app.world().get::<BackgroundColor>(e).unwrap().0,
            FIELD_BG_ACTIVE
        );

        // Flip to visible — editor must become disabled + dimmed.
        app.world_mut()
            .resource_mut::<ReplayStartupProgress>()
            .visible = true;
        app.update();
        assert!(!app.world().get::<TextInputNode>(e).unwrap().is_enabled);
        assert_eq!(
            app.world().get::<BackgroundColor>(e).unwrap().0,
            FIELD_BG_DISABLED
        );

        // Flip back — editor re-enabled and bg restored.
        app.world_mut()
            .resource_mut::<ReplayStartupProgress>()
            .visible = false;
        app.update();
        assert!(app.world().get::<TextInputNode>(e).unwrap().is_enabled);
        assert_eq!(
            app.world().get::<BackgroundColor>(e).unwrap().0,
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
                TextInputNode::default(),
                BackgroundColor(FIELD_BG_ACTIVE),
            ))
            .id();

        app.update();
        assert!(!app.world().get::<TextInputNode>(e).unwrap().is_enabled);
        assert_eq!(
            app.world().get::<BackgroundColor>(e).unwrap().0,
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
        // 入力経路の whitespace-strip は `commit_for_field` が担う（旧 CosmicTextChanged 経路の
        // 代替: `TextInputContents` は private field でテストから構築できないので純関数を直接叩く）。
        let mut app = make_app();
        let ev = commit_for_field(ScenarioStartupField::Start, "  2024-01-01  ")
            .expect("Start field yields a commit event");
        app.world_mut().send_event(ev);
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
        let ev = commit_for_field(ScenarioStartupField::InitialCash, "  1000  ")
            .expect("InitialCash field yields a commit event");
        app.world_mut().send_event(ev);
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
        let ev = commit_for_field(ScenarioStartupField::Start, "   ")
            .expect("Start field yields a commit event");
        app.world_mut().send_event(ev);
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
        let ev = commit_for_field(ScenarioStartupField::InitialCash, "   ")
            .expect("InitialCash field yields a commit event");
        app.world_mut().send_event(ev);
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

    // NOTE: cosmic 固有の DPI/render-scale テスト（`startup_field_render_scale_follows_window_dpi`）
    // と sprite-tint テスト（`startup_field_sprite_tint_is_white_with_dark_cosmic_bg`）は、
    // screen-space `bevy_ui_text_input` 化で前提（CosmicRenderScale / CosmicBackgroundColor /
    // Sprite tint）が消えたため撤去（ADR 0003 の受容済み機能後退）。
}
