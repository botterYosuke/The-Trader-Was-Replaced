//! K1 chart_candles_and_crosshair_render — 選択銘柄の OHLC / volume / latest price が
//! チャートに描画され、カーソル位置に crosshair と価格・時刻・出来高バッジが出ることを保証する（kind:render）。
//!
//! テストでは fixed fixture で window smoke または structured UI dump を取得し、candle/volume/crosshair primitives を観測する。
