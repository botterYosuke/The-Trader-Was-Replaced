//! B3 order_seeded_during_replay — Replay Run 中に戦略が約定するたびに
//! Orders パネルが更新されることを保証する（kind:state）。
//!
//! seam: BackendStatusUpdate::OrderSeeded を status seam に送信
//! 観測: LiveOrders にエントリが追加され symbol / side / status / strategy_id が正しいこと
//! be: mock
//! kind: state
//! 優先: ★★★
//!
//! 実装時の注意:
//!   GuiBridgeActor.make_order_handler() が OrderFilled を受けると
//!   RustBacktestSink.push_order() → BackendStatusUpdate::OrderSeeded となる。
//!   E2E ハーネスは Harness::send_status(OrderSeeded{...}) を送ればよい。
//!
//! 詳細は `tests/e2e/FLOWS.md` の B3 を参照。
