//! L3 prod_guard_blocks_without_env — `BACKEND_ENABLED` が未セット / `false` のとき
//! `TradingSettings::from_env()` がバックエンド接続を無効化し、`true` のときだけ有効化する
//! ことを保証する（kind:unit / env-isolated）。
//!
//! これは `src/main.rs` の ~336 行目にある `if !settings.backend_enabled { return; }` ガードの
//! 設定側コントラクトのテストである。同様の unit テストが `src/trading.rs` 内にも存在するが、
//! このファイルは E2E suite の L グループの一員として、ガードを「外から」観測する。
//!
//! 環境変数をグローバルに変更するため `#[serial]` で直列化する。

use backcast::trading::TradingSettings;
use serial_test::serial;

/// `BACKEND_ENABLED` / `BACKEND_URL` を一時的に差し替え、Drop で復元する RAII ガード。
/// `std::env::set_var` / `remove_var` は unsafe（Rust 1.80+）なため unsafe ブロックで呼ぶ。
struct EnvGuard {
    key: &'static str,
    prev: Option<String>,
}

impl EnvGuard {
    /// `key` を `value` にセットし、Drop 時に元の値へ戻す。
    fn set(key: &'static str, value: &str) -> Self {
        let prev = std::env::var(key).ok();
        // SAFETY: このテストは #[serial] で直列化されており、他スレッドと同時に
        // 環境変数を読み書きしない。Drop で復元するため漏洩しない。
        unsafe { std::env::set_var(key, value) };
        Self { key, prev }
    }

    /// `key` を削除し、Drop 時に元の値へ戻す。
    fn remove(key: &'static str) -> Self {
        let prev = std::env::var(key).ok();
        unsafe { std::env::remove_var(key) };
        Self { key, prev }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.prev {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

#[test]
#[serial]
fn l3_prod_guard_blocks_without_env() {
    // ── (1) BACKEND_ENABLED 未セット → backend_enabled == false ─────────────
    let _g1 = EnvGuard::remove("BACKEND_ENABLED");
    let settings_unset = TradingSettings::from_env();
    assert!(
        !settings_unset.backend_enabled,
        "BACKEND_ENABLED が未セットのとき backend_enabled は false のはず (got {})",
        settings_unset.backend_enabled
    );

    // ── (2) BACKEND_ENABLED=false → backend_enabled == false ─────────────────
    let _g2 = EnvGuard::set("BACKEND_ENABLED", "false");
    let settings_false = TradingSettings::from_env();
    assert!(
        !settings_false.backend_enabled,
        "BACKEND_ENABLED=false のとき backend_enabled は false のはず"
    );
    drop(_g2);

    // ── (3) BACKEND_ENABLED=0 → backend_enabled == false（"true" 以外は全て false）
    let _g3 = EnvGuard::set("BACKEND_ENABLED", "0");
    let settings_zero = TradingSettings::from_env();
    assert!(
        !settings_zero.backend_enabled,
        "BACKEND_ENABLED=0 のとき backend_enabled は false のはず"
    );
    drop(_g3);

    // ── (4) BACKEND_ENABLED=true → backend_enabled == true（接続許可）────────
    let _g4 = EnvGuard::set("BACKEND_ENABLED", "true");
    let settings_true = TradingSettings::from_env();
    assert!(
        settings_true.backend_enabled,
        "BACKEND_ENABLED=true のとき backend_enabled は true のはず"
    );
    drop(_g4);

    // ── (5) 大文字の TRUE も許容されること ──────────────────────────────────
    let _g5 = EnvGuard::set("BACKEND_ENABLED", "TRUE");
    let settings_upper = TradingSettings::from_env();
    assert!(
        settings_upper.backend_enabled,
        "BACKEND_ENABLED=TRUE（大文字）のとき backend_enabled は true のはず"
    );
    drop(_g5);
}
