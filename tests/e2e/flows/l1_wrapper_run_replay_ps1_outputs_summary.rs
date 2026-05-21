//! L1 wrapper_run_replay_ps1_outputs_summary — `scripts/run_replay.ps1` が scenario 読取・catalog 準備・
//! replay 実行を行い、run_id / run_dir / equity_points / fills_count / total_pnl を出力することを保証する（kind:integration）。
//!
//! ## darwinCI では実行不可（Windows 専用スクリプト）
//!
//! `run_replay.ps1` は PowerShell スクリプトであり、`pwsh` が darwin で使えたとしても、
//! スクリプト内部で Windows-only の `%APPDATA%` パス展開・`robocopy` などを使っている。
//! また CI 環境では PowerShell 自体が存在しない場合がほとんどである。
//!
//! このファイルはそのような事情のドキュメント stub としてのみ存在する。
//! 実行ゲートは「Windows + kabuステーション + PowerShell 7.x がそろった手動 CI 環境」であり、
//! 自動 darwin CI では **完全にスキップ** してよい。
//!
//! ### 将来の実装指針
//! - `#[test] #[ignore]` に格上げして `pwsh -NonInteractive -File scripts/run_replay.ps1 ...`
//!   を `std::process::Command` で呼ぶ。
//! - テスト環境のセットアップ: `ARTIFACTS_PATH`（catalog）と temp `--run-buffer-dir` が必要。
//! - 観測点: exit code == 0 / stdout に `"run_id"` キーを含む JSON / run_dir に
//!   `meta.json`, `equity.jsonl`, `fills.jsonl` が存在すること。
//!
//! skipped: Windows-only PowerShell script; not runnable on darwin CI.
