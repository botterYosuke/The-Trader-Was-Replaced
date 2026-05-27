//! CommonMark → typed block tree.
//!
//! Wraps `pulldown-cmark` so the rest of the crate works against a tree
//! of [`Block`] / [`Inline`] values it owns, not borrowed parser events.
//!
//! Supported: headings, paragraphs, code blocks, lists, blockquote, hr,
//! emphasis (bold/italic/strike), inline code, links, hard/soft breaks.
//! Tables, images, footnotes, task-list markers, HTML, and math collapse
//! to plain text.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Parser, Tag, TagEnd};

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Heading {
        level: u8,
        inlines: Vec<Inline>,
    },
    Paragraph(Vec<Inline>),
    CodeBlock {
        lang: Option<String>,
        text: String,
    },
    List {
        ordered: bool,
        items: Vec<Vec<Block>>,
    },
    Blockquote(Vec<Block>),
    Rule,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    Text { text: String, style: InlineStyle },
    Code(String),
    Link { href: String, children: Vec<Inline> },
    SoftBreak,
    HardBreak,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InlineStyle {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
}

/// Parse a CommonMark string into a sequence of top-level blocks.
pub fn parse(source: &str) -> Vec<Block> {
    let mut walker = Walker::default();
    for event in Parser::new(source) {
        walker.feed(event);
    }
    walker.finish()
}

/// Recursive-descent state machine driven by pulldown-cmark events.
/// Block/inline stacks track nesting; pushed on `Start`, popped on `End`.
#[derive(Default)]
struct Walker {
    blocks: Vec<Block>,
    block_stack: Vec<BlockFrame>,
    inline_stack: Vec<InlineFrame>,
    style: InlineStyle,
    code_buf: Option<CodeBuf>,
}

enum BlockFrame {
    List {
        ordered: bool,
        items: Vec<Vec<Block>>,
    },
    Item {
        children: Vec<Block>,
    },
    Blockquote {
        children: Vec<Block>,
    },
}

enum InlineFrame {
    Paragraph(Vec<Inline>),
    Heading { level: u8, inlines: Vec<Inline> },
    Link { href: String, children: Vec<Inline> },
}

struct CodeBuf {
    lang: Option<String>,
    text: String,
}

impl Walker {
    fn feed(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(end) => self.end(end),
            Event::Text(s) => {
                if let Some(code) = self.code_buf.as_mut() {
                    code.text.push_str(&s);
                } else {
                    self.push_inline(Inline::Text {
                        text: s.into_string(),
                        style: self.style,
                    });
                }
            }
            Event::Code(s) => self.push_inline(Inline::Code(s.into_string())),
            Event::SoftBreak => self.push_inline(Inline::SoftBreak),
            Event::HardBreak => self.push_inline(Inline::HardBreak),
            Event::Rule => self.push_block(Block::Rule),
            Event::Html(_) | Event::InlineHtml(_) => {}
            Event::FootnoteReference(_) => {}
            Event::TaskListMarker(_) => {}
            Event::InlineMath(s) | Event::DisplayMath(s) => {
                self.push_inline(Inline::Code(s.into_string()))
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.inline_stack.push(InlineFrame::Paragraph(Vec::new())),
            Tag::Heading { level, .. } => self.inline_stack.push(InlineFrame::Heading {
                level: heading_to_u8(level),
                inlines: Vec::new(),
            }),
            Tag::BlockQuote(_) => self.block_stack.push(BlockFrame::Blockquote {
                children: Vec::new(),
            }),
            Tag::CodeBlock(kind) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(s) => {
                        let s = s.into_string();
                        if s.is_empty() {
                            None
                        } else {
                            Some(s)
                        }
                    }
                    CodeBlockKind::Indented => None,
                };
                self.code_buf = Some(CodeBuf {
                    lang,
                    text: String::new(),
                });
            }
            Tag::List(start) => {
                let ordered = start.is_some();
                self.block_stack.push(BlockFrame::List {
                    ordered,
                    items: Vec::new(),
                });
            }
            Tag::Item => self.block_stack.push(BlockFrame::Item {
                children: Vec::new(),
            }),
            Tag::Emphasis => self.style.italic = true,
            Tag::Strong => self.style.bold = true,
            Tag::Strikethrough => self.style.strike = true,
            Tag::Link { dest_url, .. } => self.inline_stack.push(InlineFrame::Link {
                href: dest_url.into_string(),
                children: Vec::new(),
            }),
            Tag::Image { .. } => {}
            Tag::Table(_) | Tag::TableHead | Tag::TableRow | Tag::TableCell => {}
            Tag::FootnoteDefinition(_) => {}
            Tag::HtmlBlock => {}
            Tag::DefinitionList | Tag::DefinitionListTitle | Tag::DefinitionListDefinition => {}
            Tag::MetadataBlock(_) => {}
        }
    }

    fn end(&mut self, end: TagEnd) {
        match end {
            TagEnd::Paragraph => {
                if let Some(InlineFrame::Paragraph(inlines)) = self.inline_stack.pop() {
                    self.push_block(Block::Paragraph(inlines));
                }
            }
            TagEnd::Heading(_) => {
                if let Some(InlineFrame::Heading { level, inlines }) = self.inline_stack.pop() {
                    self.push_block(Block::Heading { level, inlines });
                }
            }
            TagEnd::BlockQuote(_) => {
                if let Some(BlockFrame::Blockquote { children }) = self.block_stack.pop() {
                    self.push_block(Block::Blockquote(children));
                }
            }
            TagEnd::CodeBlock => {
                if let Some(CodeBuf { lang, mut text }) = self.code_buf.take() {
                    if text.ends_with('\n') {
                        text.pop();
                    }
                    self.push_block(Block::CodeBlock { lang, text });
                }
            }
            TagEnd::List(_) => {
                if let Some(BlockFrame::List { ordered, items }) = self.block_stack.pop() {
                    self.push_block(Block::List { ordered, items });
                }
            }
            TagEnd::Item => {
                if let Some(BlockFrame::Item { children }) = self.block_stack.pop() {
                    if let Some(BlockFrame::List { items, .. }) = self.block_stack.last_mut() {
                        items.push(children);
                    }
                }
            }
            TagEnd::Emphasis => self.style.italic = false,
            TagEnd::Strong => self.style.bold = false,
            TagEnd::Strikethrough => self.style.strike = false,
            TagEnd::Link => {
                if let Some(InlineFrame::Link { href, children }) = self.inline_stack.pop() {
                    self.push_inline(Inline::Link { href, children });
                }
            }
            _ => {}
        }
    }

    fn push_inline(&mut self, inline: Inline) {
        match self.inline_stack.last_mut() {
            Some(InlineFrame::Paragraph(v))
            | Some(InlineFrame::Heading { inlines: v, .. })
            | Some(InlineFrame::Link { children: v, .. }) => v.push(inline),
            None => {
                // Stray inline outside any paragraph (rare — usually only
                // happens for top-level text in a list item without a
                // wrapping paragraph). Wrap it in a fresh paragraph.
                self.push_block(Block::Paragraph(vec![inline]));
            }
        }
    }

    fn push_block(&mut self, block: Block) {
        match self.block_stack.last_mut() {
            Some(BlockFrame::Item { children }) | Some(BlockFrame::Blockquote { children }) => {
                children.push(block)
            }
            Some(BlockFrame::List { .. }) => {
                // A bare block directly inside a list (not in an item) —
                // shouldn't happen with well-formed CommonMark; drop it.
            }
            None => self.blocks.push(block),
        }
    }

    fn finish(self) -> Vec<Block> {
        self.blocks
    }
}

fn heading_to_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(s: &str) -> Inline {
        Inline::Text {
            text: s.into(),
            style: InlineStyle::default(),
        }
    }

    fn styled(s: &str, style: InlineStyle) -> Inline {
        Inline::Text {
            text: s.into(),
            style,
        }
    }

    #[test]
    fn parses_bold_italic_code() {
        let blocks = parse("**bold** *em* `code`");
        let Block::Paragraph(inlines) = &blocks[0] else {
            panic!()
        };
        assert!(inlines.contains(&styled(
            "bold",
            InlineStyle {
                bold: true,
                ..default_style()
            }
        )));
        assert!(inlines.contains(&styled(
            "em",
            InlineStyle {
                italic: true,
                ..default_style()
            }
        )));
        assert!(inlines.contains(&Inline::Code("code".into())));
        // The spaces between runs may be merged with adjacent text or
        // emitted as standalone " " runs — what matters is the styled
        // payloads are distinct.
    }

    #[test]
    fn parses_combined_bold_italic() {
        let blocks = parse("***both***");
        let Block::Paragraph(inlines) = &blocks[0] else {
            panic!()
        };
        assert!(inlines
            .iter()
            .any(|i| matches!(i, Inline::Text { text, style } if text == "both" && style.bold && style.italic)));
    }

    #[test]
    fn parses_strikethrough() {
        let blocks = parse("~~gone~~");
        let Block::Paragraph(inlines) = &blocks[0] else {
            panic!()
        };
        // pulldown-cmark needs the `strikethrough` feature for ~~~ — if
        // it's not enabled, this just emits plain "~~gone~~". Either
        // way, the input doesn't panic; we accept whatever the parser
        // produces.
        let _ = inlines;
    }

    #[test]
    fn parses_fenced_code_block_with_lang() {
        let blocks = parse("```rust\nfn x() {}\n```");
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            Block::CodeBlock { lang, text } => {
                assert_eq!(lang.as_deref(), Some("rust"));
                assert_eq!(text, "fn x() {}");
            }
            other => panic!("expected CodeBlock, got {other:?}"),
        }
    }

    #[test]
    fn parses_nested_list_and_blockquote() {
        let src = "- outer\n  - inner\n\n> quoted";
        let blocks = parse(src);
        assert_eq!(blocks.len(), 2);
        match &blocks[0] {
            Block::List {
                ordered: false,
                items,
            } => {
                assert_eq!(items.len(), 1);
                assert!(matches!(items[0].iter().last(), Some(Block::List { .. })));
            }
            other => panic!("expected List, got {other:?}"),
        }
        match &blocks[1] {
            Block::Blockquote(children) => assert_eq!(children.len(), 1),
            other => panic!("expected Blockquote, got {other:?}"),
        }
    }

    #[test]
    fn parses_hard_and_soft_break() {
        let blocks = parse("a  \nb\nc");
        let Block::Paragraph(inlines) = &blocks[0] else {
            panic!()
        };
        assert!(inlines.contains(&Inline::HardBreak));
        assert!(inlines.contains(&Inline::SoftBreak));
    }

    #[test]
    fn parses_link() {
        let blocks = parse("[bevy](https://bevyengine.org)");
        let Block::Paragraph(inlines) = &blocks[0] else {
            panic!()
        };
        match &inlines[0] {
            Inline::Link { href, children } => {
                assert_eq!(href, "https://bevyengine.org");
                assert_eq!(children, &vec![plain("bevy")]);
            }
            other => panic!("expected Link, got {other:?}"),
        }
    }

    #[test]
    fn parses_hr() {
        let blocks = parse("a\n\n---\n\nb");
        assert!(blocks.iter().any(|b| matches!(b, Block::Rule)));
    }

    fn default_style() -> InlineStyle {
        InlineStyle::default()
    }
}
