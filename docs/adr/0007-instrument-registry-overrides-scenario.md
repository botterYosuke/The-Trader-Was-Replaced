---
status: accepted
---

# InstrumentRegistry が空でなければ ScenarioMetadata.instruments より優先する

## Context

Replay Run を開始するとき、戦略が実行する銘柄 universe を決定する経路が 2 つある。

1. **ScenarioMetadata.instruments** — シナリオファイル（JSON / TOML）に埋め込まれた銘柄リスト。バックテスト作成時点の静的な記録。
2. **InstrumentRegistry** — UI の Instrument 選択パネルで実行直前にユーザーが指定した銘柄セット。

Replay Run 開始時にどちらを使うかのルールが暗黙だったため、UI で銘柄を変更しても ScenarioMetadata の内容が使われ続ける不具合が発生した（issue #68 Slice 5-A）。

## Decision

`handle_strategy_run_system` の銘柄解決ロジックは次のルールに従う。

> **InstrumentRegistry が 1 件以上の銘柄を保持しているときは InstrumentRegistry を使用する。InstrumentRegistry が空のときのみ ScenarioMetadata.instruments にフォールバックする。**

実装箇所: `src/ui/menu_bar.rs` の `handle_strategy_run_system`。

## Why this is recorded

「UI で選択した銘柄がなぜ使われなかったのか」を将来の読者が理解できるようにするため。
優先順位のルールが 2 か所（Rust フロント / Python バック）に分散しているため、どちらを変えても壊れる可能性があり、決定を明文化しておく必要がある。

## Considered alternatives

- **ScenarioMetadata を常に使う** — シナリオの再現性は高いが、UI 操作が無視される。ユーザーが実行直前に銘柄を変更する運用フローに合わない。
- **InstrumentRegistry を常に使う（フォールバックなし）** — Registry が空の状態で Run を開始するとゼロ銘柄で実行が始まり、空振りエラーになる。既存シナリオをそのまま再生するユースケースが壊れる。
- **UI で明示的に「どちらを使うか」を選ばせる** — 設定箇所が増えてユーザー負荷が上がる。大半のケースは「UI で選んだものを使いたい」なので、暗黙のデフォルトで十分。

## Consequences

**得られる保証**

- UI の Instrument 選択パネルで選んだ銘柄が、そのまま戦略の universe になる。
- シナリオファイルをそのまま再生したい場合は InstrumentRegistry を空にしてから Run を開始すれば ScenarioMetadata.instruments が使われる。

**受け入れるトレードオフ**

- InstrumentRegistry を空にした状態で Run を開始すると ScenarioMetadata に静かにフォールバックするため、「空なのになぜ動いているのか」が直感に反することがある。将来的にはフォールバック時にログまたは UI 通知を出すことで緩和できる。
- Registry の状態はセッション間で永続化されないため、アプリ再起動後は再選択が必要。
