# Context — The-Trader-Was-Replaced

## Replay Run

戦略ファイル（`.py` + サイドカー `.json`）は必須である。
`BacktestEngine` が注文・ポジション・統計を生成し、`InstrumentRegistry` が再生対象の universe を決定する。

## Strategy Universe

Replay Run の開始時に `InstrumentRegistry` の登録内容で universe を上書きする。
`InstrumentRegistry` が空の場合は `ScenarioMetadata.instruments` にフォールバックする。
