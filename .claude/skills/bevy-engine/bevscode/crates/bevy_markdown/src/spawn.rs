//! Block tree to `bevy_ui` entity tree.
//!
//! Each [`Block`] becomes a flex child of the markdown root. Inline
//! runs become `TextSpan` children of a parent `Text` entity (how
//! `bevy_text` composes multi-style paragraphs).

use bevy::prelude::*;

use crate::highlight::{CodeHighlighter, MarkdownHighlighter, NoHighlight};
use crate::parse::{parse, Block, Inline, InlineStyle};
use crate::theme::{MarkdownColors, MarkdownFonts, MarkdownScales, MarkdownSpacing, ThemeRef};

/// Href marker on link [`TextSpan`]s for click-handler systems.
#[derive(Component, Clone, Debug)]
pub struct MarkdownLink(pub String);

/// `highlighter` defaults to [`NoHighlight`] when `None`.
pub fn spawn_markdown(
    parent: &mut ChildSpawnerCommands<'_>,
    source: &str,
    fonts: &MarkdownFonts,
    colors: &MarkdownColors,
    spacing: &MarkdownSpacing,
    scales: &MarkdownScales,
    highlighter: Option<&MarkdownHighlighter>,
) {
    let theme = ThemeRef {
        fonts,
        colors,
        spacing,
        scales,
    };
    let fallback;
    let hl: &dyn CodeHighlighter = match highlighter {
        Some(h) => &*h.0,
        None => {
            fallback = NoHighlight;
            &fallback
        }
    };
    let blocks = parse(source);
    for block in &blocks {
        spawn_block(parent, block, &theme, hl);
    }
}

fn spawn_block(
    parent: &mut ChildSpawnerCommands<'_>,
    block: &Block,
    theme: &ThemeRef<'_>,
    hl: &dyn CodeHighlighter,
) {
    match block {
        Block::Heading { level, inlines } => spawn_heading(parent, *level, inlines, theme),
        Block::Paragraph(inlines) => spawn_paragraph(parent, inlines, theme),
        Block::CodeBlock { lang, text } => {
            spawn_code_block(parent, lang.as_deref(), text, theme, hl)
        }
        Block::List { ordered, items } => spawn_list(parent, *ordered, items, theme, hl),
        Block::Blockquote(children) => spawn_blockquote(parent, children, theme, hl),
        Block::Rule => spawn_rule(parent, theme),
    }
}

fn spawn_heading(
    parent: &mut ChildSpawnerCommands<'_>,
    level: u8,
    inlines: &[Inline],
    theme: &ThemeRef<'_>,
) {
    let scale = theme.scales.0[(level.clamp(1, 6) as usize) - 1];
    let size = theme.spacing.base_font_size * scale;
    parent
        .spawn((
            Text::default(),
            TextFont {
                font: theme
                    .fonts
                    .bold
                    .clone()
                    .unwrap_or_else(|| theme.fonts.body.clone()),
                font_size: size,
                ..default()
            },
            TextColor(theme.colors.text),
            Node {
                margin: UiRect {
                    top: Val::Px(theme.spacing.block_gap * 1.5),
                    bottom: Val::Px(theme.spacing.block_gap * 0.5),
                    ..default()
                },
                ..default()
            },
        ))
        .with_children(|text_parent| {
            for inline in inlines {
                spawn_inline_span(text_parent, inline, theme, size, true);
            }
        });
}

fn spawn_paragraph(
    parent: &mut ChildSpawnerCommands<'_>,
    inlines: &[Inline],
    theme: &ThemeRef<'_>,
) {
    let size = theme.spacing.base_font_size;
    parent
        .spawn((
            Text::default(),
            TextFont {
                font: theme.fonts.body.clone(),
                font_size: size,
                ..default()
            },
            TextColor(theme.colors.text),
            Node {
                margin: UiRect {
                    bottom: Val::Px(theme.spacing.block_gap),
                    ..default()
                },
                ..default()
            },
        ))
        .with_children(|text_parent| {
            for inline in inlines {
                spawn_inline_span(text_parent, inline, theme, size, false);
            }
        });
}

fn spawn_code_block(
    parent: &mut ChildSpawnerCommands<'_>,
    lang: Option<&str>,
    text: &str,
    theme: &ThemeRef<'_>,
    hl: &dyn CodeHighlighter,
) {
    let size = theme.spacing.base_font_size;
    let border_visible = theme.colors.code_border.alpha() > 0.0;
    parent
        .spawn((
            Node {
                padding: theme.spacing.code_padding,
                margin: UiRect {
                    top: Val::Px(theme.spacing.block_gap * 0.5),
                    bottom: Val::Px(theme.spacing.block_gap),
                    ..default()
                },
                width: Val::Percent(100.0),
                border: if border_visible {
                    UiRect::all(Val::Px(1.0))
                } else {
                    UiRect::ZERO
                },
                border_radius: BorderRadius::all(Val::Px(theme.spacing.code_corner_radius)),
                ..default()
            },
            BackgroundColor(theme.colors.code_bg),
            BorderColor::all(theme.colors.code_border),
        ))
        .with_children(|outer| {
            outer
                .spawn((
                    Text::default(),
                    TextFont {
                        font: theme.fonts.mono.clone(),
                        font_size: size,
                        ..default()
                    },
                    TextColor(theme.colors.text),
                ))
                .with_children(|t| {
                    let ranges = hl.highlight(lang, text);
                    if ranges.is_empty() {
                        t.spawn((
                            TextSpan::new(text.to_string()),
                            TextFont {
                                font: theme.fonts.mono.clone(),
                                font_size: size,
                                ..default()
                            },
                            TextColor(theme.colors.text),
                        ));
                        return;
                    }
                    let mut cursor = 0usize;
                    for (range, color) in ranges {
                        let start = range.start.min(text.len()).max(cursor);
                        let end = range.end.min(text.len()).max(start);
                        if start >= end {
                            continue;
                        }
                        if cursor < start {
                            t.spawn((
                                TextSpan::new(text[cursor..start].to_string()),
                                TextFont {
                                    font: theme.fonts.mono.clone(),
                                    font_size: size,
                                    ..default()
                                },
                                TextColor(theme.colors.text),
                            ));
                        }
                        t.spawn((
                            TextSpan::new(text[start..end].to_string()),
                            TextFont {
                                font: theme.fonts.mono.clone(),
                                font_size: size,
                                ..default()
                            },
                            TextColor(color),
                        ));
                        cursor = end;
                    }
                    if cursor < text.len() {
                        t.spawn((
                            TextSpan::new(text[cursor..].to_string()),
                            TextFont {
                                font: theme.fonts.mono.clone(),
                                font_size: size,
                                ..default()
                            },
                            TextColor(theme.colors.text),
                        ));
                    }
                });
        });
}

fn spawn_list(
    parent: &mut ChildSpawnerCommands<'_>,
    ordered: bool,
    items: &[Vec<Block>],
    theme: &ThemeRef<'_>,
    hl: &dyn CodeHighlighter,
) {
    parent
        .spawn((Node {
            flex_direction: FlexDirection::Column,
            margin: UiRect {
                bottom: Val::Px(theme.spacing.block_gap),
                ..default()
            },
            ..default()
        },))
        .with_children(|list| {
            for (i, item_blocks) in items.iter().enumerate() {
                let marker = if ordered {
                    format!("{}. ", i + 1)
                } else {
                    "• ".to_string()
                };
                list.spawn((Node {
                    flex_direction: FlexDirection::Row,
                    margin: UiRect {
                        left: Val::Px(theme.spacing.list_indent),
                        ..default()
                    },
                    ..default()
                },))
                    .with_children(|row| {
                        row.spawn((
                            Text::new(marker),
                            TextFont {
                                font: theme.fonts.body.clone(),
                                font_size: theme.spacing.base_font_size,
                                ..default()
                            },
                            TextColor(theme.colors.text),
                            Node {
                                margin: UiRect {
                                    right: Val::Px(4.0),
                                    ..default()
                                },
                                ..default()
                            },
                        ));
                        row.spawn((Node {
                            flex_direction: FlexDirection::Column,
                            flex_grow: 1.0,
                            ..default()
                        },))
                            .with_children(|content| {
                                for block in item_blocks {
                                    spawn_block(content, block, theme, hl);
                                }
                            });
                    });
            }
        });
}

fn spawn_blockquote(
    parent: &mut ChildSpawnerCommands<'_>,
    children: &[Block],
    theme: &ThemeRef<'_>,
    hl: &dyn CodeHighlighter,
) {
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                border: UiRect {
                    left: Val::Px(theme.spacing.blockquote_border_width),
                    ..default()
                },
                padding: UiRect {
                    left: Val::Px(theme.spacing.block_gap),
                    ..default()
                },
                margin: UiRect {
                    bottom: Val::Px(theme.spacing.block_gap),
                    ..default()
                },
                ..default()
            },
            BorderColor::all(theme.colors.blockquote_border),
        ))
        .with_children(|q| {
            q.spawn((Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                ..default()
            },))
                .with_children(|inner| {
                    for block in children {
                        spawn_block(inner, block, theme, hl);
                    }
                });
        });
}

fn spawn_rule(parent: &mut ChildSpawnerCommands<'_>, theme: &ThemeRef<'_>) {
    parent.spawn((
        Node {
            height: Val::Px(1.0),
            width: Val::Percent(100.0),
            margin: UiRect::vertical(Val::Px(theme.spacing.block_gap)),
            ..default()
        },
        BackgroundColor(theme.colors.hr),
    ));
}

fn spawn_inline_span(
    parent: &mut ChildSpawnerCommands<'_>,
    inline: &Inline,
    theme: &ThemeRef<'_>,
    size: f32,
    heading: bool,
) {
    match inline {
        Inline::Text { text, style } => {
            parent.spawn((
                TextSpan::new(text.clone()),
                TextFont {
                    font: pick_face(*style, theme.fonts, heading),
                    font_size: size,
                    ..default()
                },
                TextColor(theme.colors.text),
            ));
        }
        Inline::Code(code) => {
            // `TextSpan` has no per-span background in Bevy 0.18, so
            // inline code uses mono font + `inline_code_bg` as foreground.
            parent.spawn((
                TextSpan::new(code.clone()),
                TextFont {
                    font: theme.fonts.mono.clone(),
                    font_size: size * 0.92,
                    ..default()
                },
                TextColor(theme.colors.inline_code_bg),
            ));
        }
        Inline::Link { href, children } => {
            for child in children {
                spawn_link_child(parent, child, theme, size, heading, href);
            }
        }
        Inline::SoftBreak => {
            parent.spawn((
                TextSpan::new(" ".to_string()),
                TextFont {
                    font: theme.fonts.body.clone(),
                    font_size: size,
                    ..default()
                },
                TextColor(theme.colors.text),
            ));
        }
        Inline::HardBreak => {
            parent.spawn((
                TextSpan::new("\n".to_string()),
                TextFont {
                    font: theme.fonts.body.clone(),
                    font_size: size,
                    ..default()
                },
                TextColor(theme.colors.text),
            ));
        }
    }
}

fn spawn_link_child(
    parent: &mut ChildSpawnerCommands<'_>,
    inline: &Inline,
    theme: &ThemeRef<'_>,
    size: f32,
    heading: bool,
    href: &str,
) {
    match inline {
        Inline::Text { text, style } => {
            parent.spawn((
                TextSpan::new(text.clone()),
                TextFont {
                    font: pick_face(*style, theme.fonts, heading),
                    font_size: size,
                    ..default()
                },
                TextColor(theme.colors.link),
                MarkdownLink(href.to_string()),
            ));
        }
        Inline::Code(code) => {
            parent.spawn((
                TextSpan::new(code.clone()),
                TextFont {
                    font: theme.fonts.mono.clone(),
                    font_size: size * 0.92,
                    ..default()
                },
                TextColor(theme.colors.link),
                MarkdownLink(href.to_string()),
            ));
        }
        Inline::Link { .. } => {
            // Nested links are non-standard; ignored.
        }
        Inline::SoftBreak | Inline::HardBreak => {
            spawn_inline_span(parent, inline, theme, size, heading);
        }
    }
}

fn pick_face(style: InlineStyle, fonts: &MarkdownFonts, heading: bool) -> Handle<Font> {
    if style.bold && style.italic {
        if let Some(h) = fonts.bold_italic.clone() {
            return h;
        }
    }
    if style.bold {
        if let Some(h) = fonts.bold.clone() {
            return h;
        }
    }
    if style.italic {
        if let Some(h) = fonts.italic.clone() {
            return h;
        }
    }
    if heading {
        if let Some(h) = fonts.bold.clone() {
            return h;
        }
    }
    fonts.body.clone()
}
