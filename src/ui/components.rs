use bevy::prelude::*;

#[derive(Resource, Default)]
pub struct WindowManager {
    pub max_z: f32,
}

#[derive(Component)]
pub struct WindowRoot;

#[derive(Component)]
pub struct TitleBar;

#[derive(Component)]
pub struct PriceDisplay;

#[derive(Component)]
pub struct StatusIndicator;

#[derive(Component, Clone, Copy)]
pub enum TradeButton {
    Buy,
    Sell,
}

#[derive(Component)]
pub struct FooterRoot;

#[derive(Component)]
pub struct ReplayTimeLabel;

#[derive(Component)]
pub struct ReplayStateBadge;

#[derive(Component)]
pub struct GrpcStatusLabel;

#[derive(Component)]
pub struct PauseResumeLabel;

#[derive(Component, Clone, Copy, Debug)]
pub enum TransportButton {
    JumpToStart,
    StepBack,
    PauseResume,
    StepForward,
    ForceStop,
}

/// Marks a speed-selector button in the footer. Holds the multiplier value (1, 2, 5, 10, 50).
#[derive(Component, Clone, Copy, Debug)]
pub struct SpeedButton(pub u32);

#[derive(Component)]
pub struct MenuBarRoot;

#[derive(Component, Clone, Copy, Debug)]
pub enum MenuButton {
    OpenStrategy,
}

#[derive(Event, Debug, Clone)]
pub struct OpenStrategyRequested {
    pub path: std::path::PathBuf,
}

#[derive(Resource, Default, Debug, Clone)]
pub struct StrategyBuffer {
    pub original_path: Option<std::path::PathBuf>,
    pub cache_path: Option<std::path::PathBuf>,
    pub source: String,
    pub dirty: bool,
}

#[derive(Component)]
pub struct StrategyStatusLabel;

#[derive(Event, Debug, Clone)]
pub struct StrategyRunRequested {
    pub cache_path: std::path::PathBuf,
}

#[derive(Component)]
pub struct SidebarRoot;

/// Single text node that shows loading / error / empty / instrument list.
#[derive(Component)]
pub struct SidebarListLabel;

#[derive(Resource, Default, Debug, Clone)]
pub struct ScenarioMetadata {
    pub schema_version: Option<u32>,
    /// Normalized instrument list (handles both "instrument" str/list and "instruments" list)
    pub instruments: Vec<String>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub granularity: Option<String>,
    pub initial_cash: Option<i64>,
}

/// 6 種類すべての floating window を区別するための種別タグ。
/// サイドバーのボタン entity と、spawn された panel root entity の両方に貼る。
/// Sub-step 1.2 で「既に spawn 済みかどうか」を判定するのにも使う。
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelKind {
    Chart,
    StrategyEditor,
    BuyingPower,
    RunResult,
    Positions,
    Orders,
}

impl PanelKind {
    /// サイドバーのボタンに表示する文字列。
    pub fn label(self) -> &'static str {
        match self {
            PanelKind::Chart => "Chart",
            PanelKind::StrategyEditor => "Strategy Editor",
            PanelKind::BuyingPower => "Buying Power",
            PanelKind::RunResult => "Run Result",
            PanelKind::Positions => "Positions",
            PanelKind::Orders => "Orders",
        }
    }
}

/// パネルボタンが押されたとき発火するイベント。
/// `panel_spawn_dispatcher_system` が受け取り、未スポーンなら spawn する。
#[derive(Event, Debug, Clone)]
pub struct PanelSpawnRequested {
    pub kind: PanelKind,
}
