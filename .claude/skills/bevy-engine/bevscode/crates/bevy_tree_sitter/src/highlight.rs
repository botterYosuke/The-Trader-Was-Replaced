//! Highlight types and query execution.

use crate::ts;
use crate::ts::{Query, QueryCursor, Tree};
use ropey::Rope;
use std::ops::Range;
use std::sync::Arc;
use streaming_iterator::StreamingIterator;

/// One contiguous run of text sharing a single tree-sitter capture.
#[derive(Clone, Debug)]
pub struct HighlightRange {
    pub byte_range: Range<usize>,
    pub capture_name: Arc<str>,
}

pub(crate) struct RopeProvider<'a>(pub(crate) &'a Rope);

pub(crate) struct RopeChunks<'a> {
    pub(crate) chunks: ropey::iter::Chunks<'a>,
}

impl<'a> ts::TextProvider<&'a [u8]> for RopeProvider<'a> {
    type I = RopeChunks<'a>;

    fn text(&mut self, node: ts::Node) -> Self::I {
        let byte_range = node.byte_range();
        let start_char = self.0.byte_to_char(byte_range.start);
        let end_char = self.0.byte_to_char(byte_range.end.min(self.0.len_bytes()));
        RopeChunks {
            chunks: self.0.slice(start_char..end_char).chunks(),
        }
    }
}

impl<'a> Iterator for RopeChunks<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        self.chunks.next().map(|s| s.as_bytes())
    }
}

pub fn highlight_ranges(
    tree: &Tree,
    rope: &Rope,
    query: &Query,
    capture_names: &[Arc<str>],
    query_cursor: &mut QueryCursor,
    byte_range: Range<usize>,
) -> Vec<HighlightRange> {
    let end_byte = byte_range.end;

    query_cursor.set_byte_range(byte_range.clone());
    // Cap in-flight match states to prevent O(n²) blowup on deeply-nested
    // patterns (long initializer lists, macro expansions). Dropped matches
    // lose their highlight color for that frame only.
    query_cursor.set_match_limit(64);

    let mut captures = query_cursor.captures(query, tree.root_node(), RopeProvider(rope));

    let mut out: Vec<HighlightRange> = Vec::new();

    while let Some((match_ref, capture_index)) = captures.next() {
        let capture = &match_ref.captures[*capture_index];
        let capture_name = match capture_names.get(capture.index as usize) {
            Some(name) => Arc::clone(name),
            None => continue,
        };
        let node_range = capture.node.byte_range();

        if node_range.end > byte_range.start && node_range.start < end_byte {
            let abs_start = node_range.start.max(byte_range.start);
            let abs_end = node_range.end.min(end_byte);
            if abs_start < abs_end {
                out.push(HighlightRange {
                    byte_range: abs_start..abs_end,
                    capture_name,
                });
            }
        }
    }

    out.sort_by_key(|r| r.byte_range.start);
    out
}
