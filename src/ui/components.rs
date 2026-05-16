use bevy::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;

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

/// Marker for the footer PauseResume Button entity (NOT the Text child).
/// `PauseResumeLabel` marks the inner Text node for label swapping; this one
/// marks the Button entity itself so `footer_pause_resume_system` can query it.
#[derive(Component)]
pub struct PauseResumeButton;

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

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuTopLevel {
    File,
    Edit,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItem {
    SaveLayout,
    SaveLayoutAs,
    LoadLayout,
    Undo,
    Redo,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuPopup(pub MenuTopLevel);

#[derive(Resource, Default)]
pub struct OpenMenu(pub Option<MenuTopLevel>);

#[derive(Resource, Default, Debug, Clone)]
pub struct StrategyBuffer {
    pub original_path: Option<PathBuf>,
    pub cache_path: Option<PathBuf>,
    /// `merge_and_flush_to_cache` が成功したときのみ書き込む読み取り専用ビュー。
    /// ステータスラベル表示とテストに使う。マージ済みソースのキャッシュは持たない。
    pub last_merged_source: Option<String>,
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

/// floating window の × ボタンに貼るマーカー。
/// Click observer がこの entity の祖先 WindowRoot を Visibility::Hidden にする。
#[derive(Component)]
pub struct CloseButton;

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
#[derive(
    Component, Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
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

/// パネル spawn の発生源を区別するための種別。
/// `panel_spawn_dispatcher_system` が WindowSpawnEdit を push するかどうかの判定に使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelSpawnSource {
    /// サイドバーのボタンなど、ユーザー操作による spawn。
    User,
    /// レイアウト JSON ロード時の自動 spawn。
    LayoutLoad,
    /// Undo/Redo による spawn。
    UndoRedo,
}

/// パネルボタンが押されたとき発火するイベント。
/// `panel_spawn_dispatcher_system` が受け取り、未スポーンなら spawn する。
#[derive(Event, Debug, Clone)]
pub struct PanelSpawnRequested {
    pub kind: PanelKind,
    pub source: PanelSpawnSource,
    /// StrategyEditor spawn 時のみ Some。blank spawn は `StrategyEditorSpawnSpec::default()` を使う。
    pub strategy_spec: Option<StrategyEditorSpawnSpec>,
}

#[derive(Event, Debug, Clone)]
pub struct UndoMenuRequested;

#[derive(Event, Debug, Clone)]
pub struct RedoMenuRequested;

// ─── Strategy Editor multi-spawn 用型群 ───────────────────────────────────

/// 各 StrategyEditor window が持つ一意キー。root entity と editor child entity の
/// 両方に貼ることで、どちらからでも region_key で対応 entity を逆引きできる。
#[derive(Component, Debug, Clone)]
pub struct StrategyEditorId {
    pub region_key: String,
}

/// root window entity にのみ置くソース断片。editor child は持たない（単一オーナー）。
#[derive(Component, Debug, Clone, Default)]
pub struct StrategyFragment {
    pub source: String,
    pub dirty: bool,
}

/// region_key の連番を管理する Resource。
/// allocate() は常に新しいキーを返す。
/// bump_to_at_least() はレイアウト復元時に既存の最大番号に合わせる。
#[derive(Resource, Default)]
pub struct RegionKeyAllocator {
    pub next: u32,
}

impl RegionKeyAllocator {
    pub fn allocate(&mut self) -> String {
        self.next += 1;
        format!("region_{:03}", self.next)
    }

    /// レイアウト復元時に既存 region_key の番号を追い越さないよう上限を合わせる。
    pub fn bump_to_at_least(&mut self, n: u32) {
        self.next = self.next.max(n);
    }
}

/// ファイルロード後に各 region_key → source を保持する一時 Resource。
/// `handle_strategy_file_load_system` が詰め、`panel_spawn_dispatcher_system` が drain する。
/// drain 後のエントリは残らないので古いロードのゴミが残るリスクがない。
#[derive(Resource, Default, Debug)]
pub struct PendingStrategyFragments {
    /// region_key → source body（マーカー行・末尾 \n を除く）
    pub by_region_key: HashMap<String, String>,
    /// このバッチを解析した .py パス。drain 時に不一致なら warn して skip する。
    pub loaded_for_path: Option<PathBuf>,
}

/// ユーザー操作またはレイアウト復元でストラテジーファイルをロードするイベント。
/// 旧 `OpenStrategyRequested` を置き換え、mode によって read→split→spawn の
/// 判断木を `handle_strategy_file_load_system` 一か所に集約する。
#[derive(Event, Debug, Clone)]
pub struct StrategyFileLoadRequested {
    pub path: PathBuf,
    pub mode: StrategyLoadMode,
}

/// ファイルロードの発生元を区別する。
/// handler 内部の分岐条件として使い、サイドカー適用・サプレスなどを切り替える。
#[derive(Debug, Clone, Copy)]
pub enum StrategyLoadMode {
    /// ユーザーが Load Layout から .py を選択したときのロードモード。サイドカーが存在すれば適用、なければ全置換。
    UserOpen,
    /// レイアウト JSON の strategy_path フィールド由来。スポーン配置はレイアウトが決定済み。
    LayoutRestore,
}

/// `PanelSpawnRequested` に同梱して `panel_spawn_dispatcher_system` に渡す引数。
/// StrategyEditor 以外の PanelKind には使わない（`strategy_spec: None` のまま）。
#[derive(Debug, Clone)]
pub struct StrategyEditorSpawnSpec {
    /// None → `RegionKeyAllocator::allocate()` で払い出す。
    pub region_key: Option<String>,
    /// None → `PendingStrategyFragments` から drain。
    /// Some("") → 明示的な空白 spawn。
    /// Some(s) → そのまま使う。
    pub source: Option<String>,
    pub layout_source: PanelSpawnSource,
}
