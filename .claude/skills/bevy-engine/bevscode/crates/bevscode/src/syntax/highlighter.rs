//! Capture-name → theme-color mapping.
//!
//! `bevy_tree_sitter` emits structural `HighlightRange`s keyed by capture
//! name (e.g. `"keyword"`, `"function.method"`). The editor's renderer wants
//! `Color`s — this module bridges the two using the editor-side
//! `SyntaxColors`. Theme hot-swap is a free side effect: callers re-run
//! `map_highlight_color` on the read path, no cache invalidation needed.

use bevy::prelude::*;

/// Map a tree-sitter capture name (or `None`, meaning "no capture") to a
/// concrete color drawn from `syntax_theme`. Unmapped categories fall back
/// to `default_color`.
pub fn map_highlight_color(
    highlight_type: Option<&str>,
    syntax_theme: &crate::settings::SyntaxColors,
    default_color: Color,
) -> Color {
    let hl_type = match highlight_type {
        Some(t) => t,
        None => return default_color,
    };

    let base_category = hl_type.split('.').next().unwrap_or(hl_type);

    match base_category {
        "keyword" | "conditional" | "repeat" | "exception" => syntax_theme.keyword,
        "function" | "method" => syntax_theme.function,
        "type" | "class" | "interface" | "struct" | "enum" => syntax_theme.type_name,
        "variable" | "parameter" | "field" => syntax_theme.variable,
        "constant" | "boolean" | "number" | "float" => syntax_theme.constant,
        "string" | "character" => syntax_theme.string,
        "comment" | "note" | "warning" | "danger" => syntax_theme.comment,
        "operator" => syntax_theme.operator,
        "punctuation" | "delimiter" | "bracket" | "special" => syntax_theme.punctuation,
        "property" | "attribute" | "tag" | "decorator" => syntax_theme.property,
        "constructor" => syntax_theme.constructor,
        "label" => syntax_theme.label,
        "escape" => syntax_theme.escape,
        "embedded" | "include" | "preproc" => syntax_theme.embedded,
        "namespace" | "module" => syntax_theme.type_name,
        _ => default_color,
    }
}
