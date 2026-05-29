//! Q2 footer.rs に raw design token の直書きが残っていないこと — #48 Step 9 回帰ガード
//!
//! ## 概要
//! #48 で `src/ui/footer.rs` を Theme token 化した（`Color::srgb` / `Color::srgba` /
//! `BTN_NORMAL` / `BTN_HOVER` / `BTN_PRESSED` / `BTN_SPEED_SELECTED` の 6 substring を
//! `theme.colors.*` / `theme.status.*` / `DynamicSpacing::*` / `LabelSize::*` に置換）。
//!
//! 後続 issue（#46 component helper / #50 bevscode 置換 / 他 UI ファイルの token 化作業）で
//! footer.rs に raw color や file-local color const が再混入したら即 fail させる回帰ガード。
//!
//! ## kind: lint
//! `std::fs::read_to_string` で `src/ui/footer.rs` を読み substring 検索する純テキスト smoke。
//! Bevy App / Harness 非依存。
//!
//! ## 検証範囲
//! - `Color::srgb` / `Color::srgba` の直書きが 0 件
//! - `BTN_NORMAL` / `BTN_HOVER` / `BTN_PRESSED` / `BTN_SPEED_SELECTED` の旧 const 参照が 0 件
//!
//! padding 数値直書きは誤検知が多いため smoke の対象外（ボタン寸法 `Val::Px(34.0)` 等は #46
//! component helper 課題で寸法 token を導入する際に別途整理）。
//!
//! ## #48 AC §J との差分
//! footer.rs の `Val::Px(28.0)` 等の **数値 padding/高さは #46 component helper で吸収する**
//! 方針（`docs/ui-theme.md §11` で明示的に温存と宣言）。本テストはその方針下での **raw
//! color** と **直書き z (`GlobalZIndex(`)** の混入だけをガードする。`GlobalZIndex` は
//! 現状 footer.rs で 0 件のため将来の再混入ガード（characterization）。

#[test]
fn q2_footer_no_raw_design_tokens() {
    let path = "src/ui/footer.rs";
    let content =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"));

    let forbidden: &[&str] = &[
        "Color::srgb",
        "Color::srgba",
        "BTN_NORMAL",
        "BTN_HOVER",
        "BTN_PRESSED",
        "BTN_SPEED_SELECTED",
        "GlobalZIndex(",
    ];

    let mut hits: Vec<&str> = Vec::new();
    for needle in forbidden {
        if content.contains(needle) {
            hits.push(needle);
        }
    }

    assert!(
        hits.is_empty(),
        "src/ui/footer.rs contains forbidden raw design tokens: {hits:?}. \
         Use theme.colors.* / theme.status.* / DynamicSpacing / LabelSize instead. \
         See docs/ui-theme.md §8 (Anti-patterns)."
    );
}
