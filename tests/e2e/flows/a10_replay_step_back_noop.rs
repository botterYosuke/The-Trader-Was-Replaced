//! A10 replay_step_back_noop — フッターの `<`（1 バック）ボタンは現状未配線であり、
//! 押しても replay clock / transport command / run state を変えないことを保証する（kind:ui）。
//!
//! テストでは StepBack button interaction を注入し、command 未送信と `TradingSession.timestamp_ms` 不変を観測する。
