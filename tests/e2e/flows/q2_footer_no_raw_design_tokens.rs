//! Q2 footer.rs に raw design token の直書きが残っていないこと — #48 Step 9 / H6 回帰ガード
//!
//! ## 概要
//! #48 で `src/ui/footer.rs` を Theme token 化した。最初の Step 9 では color のみ
//! (`Color::srgb` / `Color::srgba` / 旧 const 6 種) を対象にし、寸法 `Val::Px(<数値>)`
//! は §11 carve-out として温存していた。**#48 H6 で carve-out を撤回**し、footer.rs の
//! 寸法も `theme.layout.footer_*` 経由に統一した。本テストはその両方を回帰ガードする。
//!
//! ## kind: lint
//! `std::fs::read_to_string` で `src/ui/footer.rs` を読み substring / 正規表現検索する純テキスト smoke。
//! Bevy App / Harness 非依存。
//!
//! ## 検証範囲
//! - `Color::srgb` / `Color::srgba` の直書きが 0 件
//! - `BTN_NORMAL` / `BTN_HOVER` / `BTN_PRESSED` / `BTN_SPEED_SELECTED` の旧 const 参照が 0 件
//! - `GlobalZIndex(` の直書きが 0 件
//! - raw `Val::Px(<数値>)` が 0 件（`Val::Px(theme.…)` のみ許可、sticky anchor は `Val::ZERO`）

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

    // H6: raw `Val::Px(<数値>)` ban. `Val::Px(theme.…)` のみ許可。
    // sticky anchor 用の 0 は `Val::ZERO` を使う。
    let raw_val_px: Vec<(usize, &str)> = content
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            // strip the "Val::Px(" prefix and check if what follows starts with a digit
            line.match_indices("Val::Px(").any(|(idx, _)| {
                let after = &line[idx + "Val::Px(".len()..];
                after.chars().next().is_some_and(|c| c.is_ascii_digit())
            })
        })
        .map(|(i, line)| (i + 1, line.trim()))
        .collect();

    assert!(
        raw_val_px.is_empty(),
        "src/ui/footer.rs has raw Val::Px(<number>) literals (H6 banned): {raw_val_px:#?}. \
         Use Val::Px(theme.layout.footer_*) or Val::ZERO instead. \
         See docs/ui-theme.md §11."
    );
}
