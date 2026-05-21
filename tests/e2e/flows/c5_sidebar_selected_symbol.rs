//! C5 sidebar_selected_symbol — Instruments 行クリックで `SelectedSymbol` が更新され、
//! 対応する Chart / price display / Live subscribe 操作の対象銘柄になることを保証する（kind:ui）。
//!
//! テストでは sidebar row interaction を注入し、`SelectedSymbol` / highlighted row / chart instrument を観測する。
