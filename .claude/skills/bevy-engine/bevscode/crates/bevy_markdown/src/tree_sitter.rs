//! Tree-sitter syntax highlighter for fenced code blocks.
//!
//! Dispatches the code block's `lang` tag to a bundled `arborium`
//! grammar. One [`TreeSitterProvider`] is built and cached per
//! language on first use.
//!
//! ```rust,no_run
//! use bevy::prelude::*;
//! use bevy_markdown::prelude::*;
//! use bevy_markdown::tree_sitter::TreeSitterHighlighter;
//! use std::sync::Arc;
//!
//! # fn setup(mut commands: Commands) {
//! commands.insert_resource(MarkdownHighlighter(Arc::new(
//!     TreeSitterHighlighter::with_default_colors(),
//! )));
//! # }
//! ```

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Mutex;

use bevy::prelude::*;
use bevy_tree_sitter::arborium;
use bevy_tree_sitter::ts;
use bevy_tree_sitter::TreeSitterProvider;
use ropey::Rope;

use crate::highlight::CodeHighlighter;

#[derive(Clone, Debug)]
pub struct SyntaxColors {
    pub keyword: Color,
    pub function: Color,
    pub type_name: Color,
    pub variable: Color,
    pub constant: Color,
    pub string: Color,
    pub comment: Color,
    pub operator: Color,
    pub punctuation: Color,
    pub property: Color,
    pub constructor: Color,
    pub label: Color,
    pub escape: Color,
    pub embedded: Color,
}

impl Default for SyntaxColors {
    fn default() -> Self {
        Self {
            keyword: Color::srgb(0.78, 0.55, 0.92),
            function: Color::srgb(0.40, 0.70, 1.00),
            type_name: Color::srgb(0.90, 0.78, 0.46),
            variable: Color::srgb(0.90, 0.90, 0.92),
            constant: Color::srgb(0.95, 0.62, 0.50),
            string: Color::srgb(0.62, 0.85, 0.55),
            comment: Color::srgb(0.45, 0.50, 0.58),
            operator: Color::srgb(0.85, 0.55, 0.62),
            punctuation: Color::srgb(0.70, 0.72, 0.78),
            property: Color::srgb(0.55, 0.82, 0.85),
            constructor: Color::srgb(0.90, 0.78, 0.46),
            label: Color::srgb(0.95, 0.62, 0.50),
            escape: Color::srgb(0.95, 0.62, 0.50),
            embedded: Color::srgb(0.85, 0.55, 0.62),
        }
    }
}

impl SyntaxColors {
    pub fn color_for(&self, capture_name: &str) -> Option<Color> {
        let base = capture_name.split('.').next().unwrap_or(capture_name);
        let color = match base {
            "keyword" | "conditional" | "repeat" | "exception" => self.keyword,
            "function" | "method" => self.function,
            "type" | "class" | "interface" | "struct" | "enum" => self.type_name,
            "variable" | "parameter" | "field" => self.variable,
            "constant" | "boolean" | "number" | "float" => self.constant,
            "string" | "character" => self.string,
            "comment" | "note" | "warning" | "danger" => self.comment,
            "operator" => self.operator,
            "punctuation" | "delimiter" | "bracket" | "special" => self.punctuation,
            "property" | "attribute" | "tag" | "decorator" => self.property,
            "constructor" => self.constructor,
            "label" => self.label,
            "escape" => self.escape,
            "embedded" | "include" | "preproc" => self.embedded,
            "namespace" | "module" => self.type_name,
            _ => return None,
        };
        Some(color)
    }
}

struct Cached {
    provider: TreeSitterProvider,
    language: ts::Language,
}

pub struct TreeSitterHighlighter {
    colors: SyntaxColors,
    cache: Mutex<HashMap<&'static str, Option<Cached>>>,
}

impl TreeSitterHighlighter {
    pub fn new(colors: SyntaxColors) -> Self {
        Self {
            colors,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_default_colors() -> Self {
        Self::new(SyntaxColors::default())
    }
}

impl CodeHighlighter for TreeSitterHighlighter {
    fn highlight(&self, lang: Option<&str>, code: &str) -> Vec<(Range<usize>, Color)> {
        let Some(slot) = lang.and_then(canonical_lang) else {
            return Vec::new();
        };

        let mut cache = match self.cache.lock() {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let entry = cache.entry(slot).or_insert_with(|| build_cached(slot));
        let Some(cached) = entry.as_mut() else {
            return Vec::new();
        };

        let mut parser = ts::Parser::new();
        if parser.set_language(&cached.language).is_err() {
            return Vec::new();
        }
        let Some(tree) = parser.parse(code, None) else {
            return Vec::new();
        };

        let rope = Rope::from_str(code);
        let Some(ranges) = cached.provider.highlight_range(&tree, &rope, 0..code.len()) else {
            return Vec::new();
        };

        ranges
            .into_iter()
            .filter_map(|r| {
                self.colors
                    .color_for(&r.capture_name)
                    .map(|c| (r.byte_range, c))
            })
            .collect()
    }
}

fn canonical_lang(raw: &str) -> Option<&'static str> {
    let trimmed = raw.trim().to_ascii_lowercase();
    let canon: &'static str = match trimmed.as_str() {
        "rs" | "rust" => "rust",
        "ts" | "typescript" => "typescript",
        "tsx" => "tsx",
        "js" | "javascript" => "javascript",
        "py" | "python" => "python",
        "go" | "golang" => "go",
        "c" => "c",
        "cpp" | "c++" | "cxx" | "cc" | "hpp" => "cpp",
        "json" => "json",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "bash" | "sh" | "shell" | "zsh" => "bash",
        "html" => "html",
        "css" => "css",
        "md" | "markdown" => "markdown",
        _ => return None,
    };
    Some(canon)
}

fn build_cached(slot: &'static str) -> Option<Cached> {
    let (language, query): (ts::Language, &str) = match slot {
        "rust" => (
            arborium::lang_rust::language().into(),
            arborium::lang_rust::HIGHLIGHTS_QUERY,
        ),
        "typescript" => (
            arborium::lang_typescript::language().into(),
            arborium::lang_typescript::HIGHLIGHTS_QUERY.as_str(),
        ),
        "tsx" => (
            arborium::lang_tsx::language().into(),
            arborium::lang_tsx::HIGHLIGHTS_QUERY.as_str(),
        ),
        "javascript" => (
            arborium::lang_javascript::language().into(),
            arborium::lang_javascript::HIGHLIGHTS_QUERY,
        ),
        "python" => (
            arborium::lang_python::language().into(),
            arborium::lang_python::HIGHLIGHTS_QUERY,
        ),
        "go" => (
            arborium::lang_go::language().into(),
            arborium::lang_go::HIGHLIGHTS_QUERY,
        ),
        "c" => (
            arborium::lang_c::language().into(),
            arborium::lang_c::HIGHLIGHTS_QUERY,
        ),
        "cpp" => (
            arborium::lang_cpp::language().into(),
            arborium::lang_cpp::HIGHLIGHTS_QUERY.as_str(),
        ),
        "json" => (
            arborium::lang_json::language().into(),
            arborium::lang_json::HIGHLIGHTS_QUERY,
        ),
        "toml" => (
            arborium::lang_toml::language().into(),
            arborium::lang_toml::HIGHLIGHTS_QUERY,
        ),
        "yaml" => (
            arborium::lang_yaml::language().into(),
            arborium::lang_yaml::HIGHLIGHTS_QUERY,
        ),
        "bash" => (
            arborium::lang_bash::language().into(),
            arborium::lang_bash::HIGHLIGHTS_QUERY,
        ),
        "html" => (
            arborium::lang_html::language().into(),
            arborium::lang_html::HIGHLIGHTS_QUERY,
        ),
        "css" => (
            arborium::lang_css::language().into(),
            arborium::lang_css::HIGHLIGHTS_QUERY,
        ),
        "markdown" => (
            arborium::lang_markdown::language().into(),
            arborium::lang_markdown::HIGHLIGHTS_QUERY,
        ),
        _ => return None,
    };
    let mut provider = TreeSitterProvider::new();
    provider.set_query(query, language.clone()).ok()?;
    Some(Cached { provider, language })
}
