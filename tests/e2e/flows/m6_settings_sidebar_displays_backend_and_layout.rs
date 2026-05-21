//! M6 settings_sidebar_displays_backend_and_layout — **SKIPPED: not yet testable headless**
//!
//! ## 調査結果
//!
//! `src/ui/sidebar.rs` の Settings セクション（~85-98 行）は以下のハードコード文字列を
//! 持つ静的スタブであり、`BackendStatus` / `AutoSaveState` / `TradingSettings` などの
//! ECS リソースには接続されていない：
//!
//! ```rust
//! parent.spawn((
//!     Text::new("Theme: Dark\nBackend: localhost:19876\nSave Layout: —"),
//!     …
//! ));
//! ```
//!
//! そのため「backend 接続状態が変化したときに表示が切り替わる」ことは現時点で
//! 本番コードとして実装されておらず、headless テストで状態変化を注入しても
//! UI 側の Text node が更新されることはない。
//!
//! ## テスト化の条件
//!
//! 以下のいずれかが実装されたときに本テストを実装する:
//! - `update_settings_sidebar_system` など、`BackendStatus` or `TradingSettings` を
//!   購読して Settings セクション内の `Text` を書き換える system が存在する
//! - Settings 内の各フィールド用マーカーコンポーネント（例 `SettingsBackendText`）が
//!   `components.rs` に定義される
//!
//! それまでは本ファイルを空のスタブとして残し、実装追従のトラッカーとする。
