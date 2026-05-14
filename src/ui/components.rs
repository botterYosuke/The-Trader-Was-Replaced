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

#[derive(Component, Clone, Copy, Debug)]
pub enum TransportButton {
    JumpToStart,
    StepBack,
    PauseResume,
    StepForward,
    Run,
}

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
