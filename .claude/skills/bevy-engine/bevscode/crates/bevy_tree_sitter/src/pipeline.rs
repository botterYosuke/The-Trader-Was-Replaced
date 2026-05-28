//! Component-driven async tree-sitter parsing.

use bevy_ecs::prelude::*;
use bevy_tasks::{AsyncComputeTaskPool, Task};
use ropey::Rope;
use std::sync::Arc;

use crate::language::TreeSitterGrammar;
use crate::tree_sitter::{build_parser, RopeReader};
use crate::ts;

/// Buffer interface for the parse pipeline.
pub trait ParseSource: Send + Sync + 'static {
    /// A bump triggers a re-parse.
    fn content_version(&self) -> u64;

    fn snapshot(&self) -> Rope;

    fn apply_edit(&self, _edit: ts::InputEdit) {}
}

/// Not Reflect: wraps `dyn`.
#[derive(Component, Clone)]
pub struct ParseSourceComp(pub Arc<dyn ParseSource>);

impl ParseSourceComp {
    pub fn new<T: ParseSource>(value: T) -> Self {
        Self(Arc::new(value))
    }
}

/// Not Reflect: `ts::Tree` owns FFI-side state.
#[derive(Component, Default)]
#[require(ParseState)]
pub struct SyntaxTree {
    pub tree: Option<ts::Tree>,
    pub content_version: u64,
    pub tree_version: u64,
    /// `None` means a full rehighlight is needed.
    pub dirty_rows: Option<(u32, u32)>,
}

impl SyntaxTree {
    pub fn clear(&mut self) {
        self.tree = None;
        self.content_version = 0;
        self.tree_version = self.tree_version.wrapping_add(1);
    }
}

#[derive(Component)]
pub(crate) enum ParseState {
    Idle(Option<ts::Parser>),
    InFlight {
        task: Task<(Option<ts::Tree>, ts::Parser)>,
        content_version: u64,
        dirty_rows: Option<(u32, u32)>,
    },
}

impl Default for ParseState {
    fn default() -> Self {
        Self::Idle(None)
    }
}

pub(crate) fn parse_dirty(
    mut targets: Query<(
        &TreeSitterGrammar,
        &ParseSourceComp,
        &mut SyntaxTree,
        &mut ParseState,
    )>,
) {
    for (grammar_comp, source, mut syntax, mut state) in targets.iter_mut() {
        match &mut *state {
            ParseState::Idle(ref mut stored_parser) => {
                let source_version = source.0.content_version();
                if source_version == syntax.content_version {
                    continue;
                }

                let grammar = grammar_comp.grammar.clone();

                let parser = match stored_parser.take() {
                    Some(p) => p,
                    None => match build_parser(&grammar) {
                        Some(p) => p,
                        None => continue,
                    },
                };

                let rope = source.0.snapshot();
                let cached_tree = syntax.tree.clone();
                let dirty_rows = syntax.bypass_change_detection().dirty_rows;
                let task = AsyncComputeTaskPool::get()
                    .spawn(async move { parse_tree_async(parser, rope, cached_tree) });

                *state = ParseState::InFlight {
                    task,
                    content_version: source_version,
                    dirty_rows,
                };
            }
            ParseState::InFlight {
                task,
                content_version,
                dirty_rows,
            } => {
                let Some((tree, parser)) =
                    futures_lite::future::block_on(futures_lite::future::poll_once(task))
                else {
                    continue;
                };

                let content_version = *content_version;
                let dirty_rows = *dirty_rows;
                *state = ParseState::Idle(Some(parser));

                if let Some(tree) = tree {
                    syntax.tree = Some(tree);
                    syntax.content_version = content_version;
                    syntax.tree_version = syntax.tree_version.wrapping_add(1);
                    syntax.dirty_rows = dirty_rows;
                } else {
                    let s = syntax.bypass_change_detection();
                    s.content_version = content_version;
                }
            }
        }
    }
}

fn parse_tree_async(
    mut parser: ts::Parser,
    rope: Rope,
    cached_tree: Option<ts::Tree>,
) -> (Option<ts::Tree>, ts::Parser) {
    let mut reader = RopeReader::new(&rope);
    let mut callback =
        |byte_offset: usize, _position: ts::Point| -> &[u8] { reader.read(byte_offset) };

    let tree = parser.parse_with_options(&mut callback, cached_tree.as_ref(), None);
    (tree, parser)
}

pub fn byte_to_point(rope: &Rope, byte_offset: usize) -> ts::Point {
    let byte_offset = byte_offset.min(rope.len_bytes());
    let char_offset = rope.byte_to_char(byte_offset);
    let line = rope.char_to_line(char_offset);
    let line_start_char = rope.line_to_char(line);
    let line_start_byte = rope.char_to_byte(line_start_char);
    let column_byte = byte_offset - line_start_byte;
    ts::Point::new(line, column_byte)
}
