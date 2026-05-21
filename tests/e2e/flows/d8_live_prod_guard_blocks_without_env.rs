//! D8 live_prod_guard_blocks_without_env — Tachibana / kabu の Prod 接続は許可環境変数なしでは遮断され、
//! 許可された場合だけ live venue 接続へ進むことを保証する（kind:integration/manual-gate）。
//!
//! テストでは env isolated backend または venue connect command seam を使い、blocked error / no live send / allowed path を観測する。
