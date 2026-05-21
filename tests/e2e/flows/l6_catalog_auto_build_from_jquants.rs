//! L6 catalog_auto_build_from_jquants — J-Quants CSV source から `scripts/build_catalog_batch.py` が
//! ParquetDataCatalog を自動構築し、その catalog を使った replay が成功することを保証する（kind:integration）。
//!
//! ## データ依存性（CI では実行不可）
//!
//! このフローは `DEV_J_QUANTS_CACHE` 環境変数が指す J-Quants CSV.gz データディレクトリを必要とする。
//! そのデータは本番環境のローカルキャッシュであり、CI リポジトリには含まれない。
//!
//! ### 実行ゲート
//! `DEV_J_QUANTS_CACHE` が存在する場合のみ `#[ignore]` を外して実行する（`cargo test -- --ignored`）。
//! 環境変数が未設定の場合は即 return してテストはパスする（CI では常にこちら）。
//!
//! ### 実行手順（ローカル開発環境）
//! ```sh
//! export DEV_J_QUANTS_CACHE=/path/to/jquants-cache
//! export ARTIFACTS_PATH=/tmp/backcast-artifacts
//! cargo test --test e2e_replay l6_ -- --ignored
//! ```
//!
//! ### 観測点（実行時）
//! 1. `build_catalog_batch.py` が exit code 0 で完了する。
//! 2. `$ARTIFACTS_PATH/jquants-catalog` ディレクトリが作成される。
//! 3. catalog に parquet ファイルが 1 つ以上存在する。
//! 4. `python -m engine.strategy_replay run --catalog <catalog_dir>` が success で終了する。

use std::process::Command;

#[test]
#[ignore = "DEV_J_QUANTS_CACHE データが必要。ローカル開発環境でのみ実行する (cargo test -- --ignored)"]
fn l6_catalog_auto_build_from_jquants() {
    // DEV_J_QUANTS_CACHE が未設定なら即スキップ（二重安全策: #[ignore] で通常は来ない）。
    let jquants_cache = match std::env::var("DEV_J_QUANTS_CACHE") {
        Ok(v) if !v.is_empty() => v,
        _ => {
            eprintln!(
                "l6: DEV_J_QUANTS_CACHE が未設定のためスキップ \
                 (J-Quants CSV データがないと実行できない)"
            );
            return;
        }
    };

    let artifacts_dir = tempfile::tempdir().expect("tempdir");
    let artifacts_path = artifacts_dir.path().to_str().unwrap().to_string();
    let catalog_dir = artifacts_dir.path().join("jquants-catalog");

    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));

    // ── Step 1: build_catalog_batch.py を実行する ────────────────────────────
    let build_out = Command::new("uv")
        .args(["run", "python", "scripts/build_catalog_batch.py"])
        .env("DEV_J_QUANTS_CACHE", &jquants_cache)
        .env("ARTIFACTS_PATH", &artifacts_path)
        .current_dir(manifest_dir)
        .output()
        .expect("uv run python scripts/build_catalog_batch.py を起動できなかった");

    let build_stdout = String::from_utf8_lossy(&build_out.stdout);
    let build_stderr = String::from_utf8_lossy(&build_out.stderr);

    assert!(
        build_out.status.success(),
        "build_catalog_batch.py が非ゼロで終了した\nstdout:\n{build_stdout}\nstderr:\n{build_stderr}"
    );

    // ── Step 2: catalog ディレクトリが作られていること ───────────────────────
    assert!(
        catalog_dir.exists(),
        "catalog ディレクトリが作られていない: {catalog_dir:?}"
    );

    // ── Step 3: parquet ファイルが 1 つ以上あること（再帰的に探索）────────────
    fn count_parquet(dir: &std::path::Path) -> usize {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return 0;
        };
        let mut count = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                count += count_parquet(&path);
            } else if path.extension().map(|x| x == "parquet").unwrap_or(false) {
                count += 1;
            }
        }
        count
    }
    let parquet_count = count_parquet(&catalog_dir);
    assert!(
        parquet_count > 0,
        "catalog に parquet ファイルがない: {catalog_dir:?}"
    );

    // ── Step 4: catalog を使った最小 replay が成功すること ───────────────────
    // フィクスチャ戦略（サイドカー形式）を tempdir に作る。
    let strat_dir = tempfile::tempdir().expect("tempdir");
    let strategy_py = strat_dir.path().join("fixture_strat.py");
    let strategy_json = strat_dir.path().join("fixture_strat.json");
    let run_buffer_dir = strat_dir.path().join("run-buffer");

    std::fs::write(
        &strategy_py,
        r#"from nautilus_trader.trading.strategy import Strategy

class FixtureStrat(Strategy):
    def __init__(self, config=None):
        super().__init__(config)
    def on_bar(self, bar):
        pass
"#,
    )
    .unwrap();

    // catalog に存在するはずの最初の銘柄を catalog から推測するのは難しいため、
    // 汎用的に 7203.TSE を試みる（ない場合は replay が失敗してここで検知できる）。
    let sidecar = serde_json::json!({
        "instrument": "7203.TSE",
        "start": "2025-01-06",
        "end": "2025-01-10",
        "granularity": "Daily",
        "initial_cash": 1_000_000
    });
    std::fs::write(
        &strategy_json,
        serde_json::to_string(&sidecar).unwrap(),
    )
    .unwrap();

    let replay_out = Command::new("uv")
        .args([
            "run",
            "python",
            "-m",
            "engine.strategy_replay",
            "run",
            "--strategy",
            strategy_py.to_str().unwrap(),
            "--catalog",
            catalog_dir.to_str().unwrap(),
            "--run-buffer-dir",
            run_buffer_dir.to_str().unwrap(),
        ])
        .current_dir(manifest_dir)
        .output()
        .expect("uv run python -m engine.strategy_replay run --catalog を起動できなかった");

    let replay_stdout = String::from_utf8_lossy(&replay_out.stdout);
    let replay_stderr = String::from_utf8_lossy(&replay_out.stderr);

    assert!(
        replay_out.status.success(),
        "catalog を使った replay が失敗した\nstdout:\n{replay_stdout}\nstderr:\n{replay_stderr}"
    );

    // stdout に run_id が含まれること。
    assert!(
        replay_stdout.contains("run_id"),
        "replay stdout に run_id が見当たらない: {replay_stdout}"
    );
}
