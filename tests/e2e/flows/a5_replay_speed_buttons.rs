//! A5 replay_speed_buttons — Replay モードの 1x / 2x / 5x / 10x / 50x 速度ボタンが
//! 選択倍率を backend へ送信し、Replay 以外では表示されないことを保証する（kind:ui）。
//!
//! テストでは speed button interaction と mode state を注入し、`SetSpeed` command / selected styling / visibility を観測する。
