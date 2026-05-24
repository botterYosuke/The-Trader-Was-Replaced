//! A12 replay_precision_mismatch_surfaced — precision 不一致 catalog の Run 失敗が
//! UI に「実原因」として出ること（GH #34）。
//!
//! 共有 catalog（standard-precision 8-byte）を high-precision(16-byte) nautilus で
//! 読むと、従来は backend が nautilus 内部で `.unwrap()` panic → プロセス丸ごと
//! abort し、UI には `transport error` しか出なかった。backend 側 preflight
//! (`nautilus_catalog_loader._assert_catalog_precision_compatible`) が query 前に
//! `CatalogPrecisionMismatchError` を raise するよう直したので、backend は生きたまま
//! `RunFailed{error}` を status seam に押し戻す。ここでは ECS 側がその error 文字列を
//! `RunState::Failed{error}` にそのまま surface する（`transport error` に握り潰さない）
//! ことを確認する。backend が落ちないこと自体は Python 側 pytest が担保する。
//! 詳細は `tests/e2e/FLOWS.md` の A12 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, RunState};

#[test]
fn a12_replay_precision_mismatch_surfaced() {
    let mut h = Harness::new();
    let startup_id = h.run_via_ui();
    h.drain_commands();
    assert_eq!(h.run_state(), RunState::Running);

    // backend が preflight で弾いた typed error をそのまま押し戻す。
    let backend_error =
        "1301.TSE: Catalog precision mismatch: stores fixed_size_binary[8] \
         but the running nautilus build expects PRECISION_BYTES=16. (GH #34)"
            .to_string();
    h.send_status(BackendStatusUpdate::RunFailed {
        startup_id: Some(startup_id),
        error: backend_error.clone(),
    });

    match h.run_state() {
        RunState::Failed { error } => {
            assert!(
                error.contains("precision mismatch"),
                "real cause not surfaced (regressed to opaque error?): {error}"
            );
            assert!(
                !error.contains("transport error"),
                "must not collapse to transport error: {error}"
            );
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}
