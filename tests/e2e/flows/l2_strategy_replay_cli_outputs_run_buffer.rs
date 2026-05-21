//! L2 strategy_replay_cli_outputs_run_buffer — `python -m engine.strategy_replay run` が
//! `--bars-json` で実行でき、run_id / run_dir / equity_points / fills_count / total_pnl を
//! stdout JSON として出力し、run-buffer ディレクトリにファイル群を書き出すことを保証する（kind:integration）。
//!
//! `uv run` でプロジェクトの venv を使い、catalog なしで動く `--bars-json` フィクスチャを注入する。
//! 戦略ファイルと bars JSON は tempdir に生成する（実 catalog 不要）。
//!
//! ## 前提条件
//! - `uv` がシステムの `PATH` に存在すること（CI で `uv` がなければ `#[ignore]` に落ちる）。
//! - `uv run python -m engine.strategy_replay` が実行できること。
//! - `nautilus_trader` 依存が venv に入っていること（`uv sync` 済み）。
//!
//! これらが満たされない環境では `STRATEGY_REPLAY_CLI_INTEGRATION=1` 未設定でスキップする。

use std::process::Command;

/// `uv` バイナリが PATH にあるかを確認するヘルパー。
fn has_uv() -> bool {
    Command::new("uv")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn l2_strategy_replay_cli_outputs_run_buffer() {
    // uv が無い環境ではこのテストを実行できない。
    // 環境変数を明示的にセットした CI のみで走らせる意図。
    if std::env::var("STRATEGY_REPLAY_CLI_INTEGRATION").is_err() {
        eprintln!(
            "l2: STRATEGY_REPLAY_CLI_INTEGRATION が未設定のためスキップ \
             (uv + nautilus_trader venv を要求)"
        );
        return;
    }
    if !has_uv() {
        eprintln!("l2: uv が PATH にないためスキップ");
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");

    // ── bars JSON フィクスチャ ───────────────────────────────────────────────
    // `_load_bars_from_json` が期待する形式:
    // { "INSTRUMENT_ID": [ { open, high, low, close, volume, ts_event, ts_init, granularity }, ... ] }
    let ts_base: i64 = 1_736_118_000_000_000_000; // 2025-01-06 09:00:00 JST (nanoseconds)
    let day_ns: i64 = 86_400_000_000_000;
    let bars: Vec<serde_json::Value> = (0..5)
        .map(|i| {
            serde_json::json!({
                "open": 3000 + i * 10,
                "high": 3020 + i * 10,
                "low":  2990 + i * 10,
                "close": 3010 + i * 10,
                "volume": 1000,
                "ts_event": ts_base + i as i64 * day_ns,
                "ts_init":  ts_base + i as i64 * day_ns,
                "granularity": "Daily"
            })
        })
        .collect();
    let bars_json = serde_json::json!({ "7203.TSE": bars });
    let bars_json_path = dir.path().join("bars.json");
    std::fs::write(&bars_json_path, serde_json::to_string(&bars_json).unwrap()).unwrap();

    // ── 戦略フィクスチャ（サイドカー JSON から scenario を読む v3 形式） ────
    // strategy_loader は sidecar `<strategy>.json` を優先するため、.py は最小限で良い。
    let strategy_py = dir.path().join("test_strat.py");
    let strategy_json = dir.path().join("test_strat.json");

    // nautilus_trader.trading.strategy.Strategy を継承する最小戦略。
    std::fs::write(
        &strategy_py,
        r#"from nautilus_trader.trading.strategy import Strategy

class TestStrat(Strategy):
    def __init__(self, config=None):
        super().__init__(config)
    def on_bar(self, bar):
        pass
"#,
    )
    .unwrap();

    // サイドカー: instrument / start / end / granularity / initial_cash の v3 形式。
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

    let run_buffer_dir = dir.path().join("run-buffer");

    // ── CLI 呼び出し ─────────────────────────────────────────────────────────
    let output = Command::new("uv")
        .args([
            "run",
            "python",
            "-m",
            "engine.strategy_replay",
            "run",
            "--strategy",
            strategy_py.to_str().unwrap(),
            "--bars-json",
            bars_json_path.to_str().unwrap(),
            "--run-buffer-dir",
            run_buffer_dir.to_str().unwrap(),
        ])
        .current_dir(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
        )
        .output()
        .expect("uv run python コマンドを起動できなかった");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "python -m engine.strategy_replay run が非ゼロで終了した\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // stdout 末尾に JSON サマリが出力される（ログ行に続いて）。
    // 最後の `{` 〜 `}` ブロックを JSON としてパースする。
    let json_start = stdout
        .rfind('{')
        .expect("stdout に JSON オブジェクトが見当たらない");
    let json_str = &stdout[json_start..];
    let summary: serde_json::Value =
        serde_json::from_str(json_str).expect("stdout の末尾 JSON がパースできない");

    assert!(
        summary.get("run_id").and_then(|v| v.as_str()).is_some(),
        "summary に run_id がない: {summary}"
    );
    assert!(
        summary.get("run_dir").and_then(|v| v.as_str()).is_some(),
        "summary に run_dir がない: {summary}"
    );
    assert!(
        summary.get("equity_points").is_some(),
        "summary に equity_points がない: {summary}"
    );
    assert!(
        summary.get("fills_count").is_some(),
        "summary に fills_count がない: {summary}"
    );
    assert!(
        summary.get("total_pnl").is_some(),
        "summary に total_pnl がない: {summary}"
    );

    // run_dir にファイルが作られていること。
    let run_dir = std::path::Path::new(
        summary["run_dir"]
            .as_str()
            .expect("run_dir is a string"),
    );
    assert!(
        run_dir.join("meta.json").exists(),
        "run_dir に meta.json がない: {run_dir:?}"
    );
    assert!(
        run_dir.join("equity.jsonl").exists(),
        "run_dir に equity.jsonl がない: {run_dir:?}"
    );
}
