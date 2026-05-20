use syntect::easy::HighlightLines;
use syntect::highlighting::{Style, ThemeSet};
use syntect::parsing::SyntaxSet;

fn main() {
    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();

    // 1. .py 拡張子が引けること
    let syntax = ss
        .find_syntax_by_extension("py")
        .expect("python syntax not found");
    println!("syntax name = {}", syntax.name);

    // 2. 使えるテーマ名を全部出す（base16-mocha.dark があるか目視確認）
    let mut names: Vec<&String> = ts.themes.keys().collect();
    names.sort();
    println!("themes = {:?}", names);
    let theme = ts
        .themes
        .get("base16-mocha.dark")
        .expect("base16-mocha.dark not found");

    // 3. 1 行ハイライトして (Style, &str) が返ること。Style.foreground が r,g,b,a:u8 であること
    let mut h = HighlightLines::new(syntax, theme);
    let line = "def foo(x):  # comment\n";
    let ranges: Vec<(Style, &str)> = h.highlight_line(line, &ss).expect("highlight_line failed");
    for (style, text) in &ranges {
        let c = style.foreground;
        println!("  fg=({:>3},{:>3},{:>3},{:>3}) {:?}", c.r, c.g, c.b, c.a, text);
    }
    println!("OK: {} spans", ranges.len());
}
