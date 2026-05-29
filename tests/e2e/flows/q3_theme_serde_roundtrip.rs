//! Q3 `Theme` serde round-trip — #48 plan §Step 4 (O) 回帰ガード
//!
//! ## 概要
//! plan §Step 4 で `src/ui/theme/{mod,scale,spacing,typography,elevation}.rs` の
//! 全 struct/enum に `#[derive(serde::Serialize, serde::Deserialize)]` を追加し、
//! `Theme::default()` を `serde_json` で round-trip しても `colors.background` 等が
//! 同値を保つことを保証する。
//!
//! ## kind: lint (smoke)
//! Bevy App / Harness 非依存の単純な構造体 round-trip。
//!
//! ## 状態: GREEN (Slice 4b + Finding 4 強化)
//! Slice 4b で serde derive 追加済み・round-trip 実装済み。
//! Finding 4 (HIGH) 対応: `colors.background` 単体 assert は silent drop
//! (例: `Typography` / `ColorScales` / `SpacingTokens` / `ElevationTokens` /
//! `PlayerColors` / `StatusColors` / `SyntaxColors` / `Radius` / `Layout` /
//! `Appearance` のいずれかに `#[serde(skip)]` が混入) を検知できないため、
//! `serde_json::Value` で全 tree 比較する gate を追加する。
//! 既存の `colors.background` assert は意図表明用 anchor として残す。
//! codex Medium 対応: non-default fixture (`Light` + `Compact`) で
//! `#[serde(skip)]` 混入 silent pass を gate。

use backcast::ui::theme::{Appearance, Theme, spacing::UiDensity};

#[test]
fn q3_theme_serde_roundtrip() {
    // Slice 4b GREEN: derive 追加済み。Theme::default() を JSON で round-trip して
    // 全 sub-struct が同値で戻ることを確認する。
    let theme = Theme::default();
    let json = serde_json::to_string(&theme).expect("serialize Theme");
    let restored: Theme = serde_json::from_str(&json).expect("deserialize Theme");

    // Anchor: 意図表明用の明示 assert (Finding 4 前の元 gate)。
    assert_eq!(
        restored.colors.background,
        theme.colors.background,
        "Theme::default().colors.background should survive serde_json round-trip"
    );

    // Finding 4 (HIGH) 強化: 全 sub-struct tree を Value 化して比較する。
    // どこかの field/struct に #[serde(skip)] が混入したら ここで落ちる。
    let theme_value = serde_json::to_value(&theme).expect("Theme -> Value");
    let restored_value = serde_json::to_value(&restored).expect("restored -> Value");
    assert_eq!(
        restored_value, theme_value,
        "Theme JSON round-trip should preserve every sub-struct tree \
         (scale/spacing/typography/elevation/players/status/syntax/radius/layout/appearance)"
    );
}

/// codex Medium finding 対応: Theme::default() 固定 fixture では
/// `#[serde(skip)]` が混入しても両側が同じ default fallback に落ち、
/// Value 比較も silent pass する。non-default value を 2 field 載せて
/// drift を gate する。
///
/// - `appearance = Light` (Default::default() は Dark): Appearance enum / Theme.appearance
///   フィールドが skip されたら restored.appearance は Dark に戻り fail。
/// - `spacing.density = Compact` (Default::default() は Default): SpacingTokens /
///   UiDensity / Theme.spacing が skip されたら restored.spacing.density は
///   Default に戻り fail。
#[test]
fn q3_theme_serde_roundtrip_non_default_fields() {
    let mut theme = Theme::default();
    theme.appearance = Appearance::Light;
    theme.spacing.density = UiDensity::Compact;

    let json = serde_json::to_string(&theme).expect("serialize non-default Theme");
    let restored: Theme = serde_json::from_str(&json).expect("deserialize non-default Theme");

    assert_eq!(
        restored.appearance,
        Appearance::Light,
        "Theme.appearance must survive serde round-trip (gate against #[serde(skip)] on appearance / Appearance)"
    );
    assert_eq!(
        restored.spacing.density,
        UiDensity::Compact,
        "Theme.spacing.density must survive serde round-trip (gate against #[serde(skip)] on spacing / SpacingTokens / UiDensity)"
    );

    // 全 tree も Value 比較で非デフォルト値を含めて gate。
    let theme_value = serde_json::to_value(&theme).expect("Theme -> Value");
    let restored_value = serde_json::to_value(&restored).expect("restored -> Value");
    assert_eq!(
        restored_value, theme_value,
        "non-default Theme JSON round-trip should preserve every sub-struct tree"
    );
}
