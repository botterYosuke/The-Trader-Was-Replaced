use std::path::PathBuf;
use serde::{Deserialize, Serialize};

pub const APP_STATE_SCHEMA_VERSION: u32 = 1;
/// config dir 内のサブディレクトリ名
const APP_CONFIG_DIR: &str = "the-trader-was-replaced";
/// 設定ファイル名
const APP_STATE_FILE: &str = "app_state.json";

/// アプリ起動をまたいで永続化するシンプルな状態。
/// `dirs::config_dir()` 配下に JSON として保存される。
///
/// `#[serde(default)]` により古い JSON (フィールドなし) との後方互換を確保。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    /// 最後に開いた strategy ファイルのパス。
    /// 起動時にこのパスが存在すれば自動ロードを試みる。
    #[serde(default)]
    pub last_strategy_path: Option<PathBuf>,
}

fn default_schema_version() -> u32 {
    APP_STATE_SCHEMA_VERSION
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            schema_version: APP_STATE_SCHEMA_VERSION,
            last_strategy_path: None,
        }
    }
}

/// `dirs::config_dir()` → `the-trader-was-replaced/app_state.json` のパスを返す。
/// `dirs::config_dir()` が None のとき（CI 環境など）は None を返す。
pub fn app_state_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join(APP_CONFIG_DIR).join(APP_STATE_FILE))
}

/// 設定ファイルを読んで `AppState` を返す。
/// ファイルが存在しない / パース失敗のときは `AppState::default()` を返す。
pub fn load_app_state() -> AppState {
    let Some(path) = app_state_path() else {
        return AppState::default();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return AppState::default();
    };
    serde_json::from_str::<AppState>(&text).unwrap_or_default()
}

/// `AppState` を設定ファイルに書き込む。
/// ディレクトリが存在しなければ作成する。
pub fn save_app_state(state: &AppState) -> std::io::Result<()> {
    let path = app_state_path()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "config_dir not found"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&path, json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_state_round_trip() {
        let state = AppState {
            schema_version: APP_STATE_SCHEMA_VERSION,
            last_strategy_path: Some(PathBuf::from("/some/path/strategy.py")),
        };
        let json = serde_json::to_string_pretty(&state).unwrap();
        let restored: AppState = serde_json::from_str(&json).unwrap();
        assert_eq!(state.schema_version, restored.schema_version);
        assert_eq!(state.last_strategy_path, restored.last_strategy_path);
    }

    #[test]
    fn app_state_default_when_missing_fields() {
        // フィールドなしの古い JSON でも壊れないことを確認
        let json = r#"{"schema_version": 1}"#;
        let state: AppState = serde_json::from_str(json).unwrap();
        assert_eq!(state.last_strategy_path, None);
    }
}
