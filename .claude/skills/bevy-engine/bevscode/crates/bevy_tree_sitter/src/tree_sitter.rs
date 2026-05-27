use crate::highlight::{highlight_ranges, HighlightRange};
use crate::ts::{Language, Parser, Query, QueryCursor, Tree};
use ropey::Rope;
use std::ops::Range;
use std::sync::Arc;

/// Seeking backwards resets the chunk iterator.
pub(crate) struct RopeReader<'a> {
    rope: &'a Rope,
    chunks: ropey::iter::Chunks<'a>,
    current_chunk: &'a [u8],
    total_byte_offset: usize,
}

impl<'a> RopeReader<'a> {
    pub(crate) fn new(rope: &'a Rope) -> Self {
        let mut chunks = rope.chunks();
        let current_chunk = chunks.next().map(|s| s.as_bytes()).unwrap_or(b"");
        Self {
            rope,
            chunks,
            current_chunk,
            total_byte_offset: 0,
        }
    }

    pub(crate) fn read(&mut self, byte_offset: usize) -> &'a [u8] {
        if byte_offset < self.total_byte_offset {
            *self = Self::new(self.rope);
        }

        while self.total_byte_offset + self.current_chunk.len() <= byte_offset {
            self.total_byte_offset += self.current_chunk.len();
            self.current_chunk = self.chunks.next().map(|s| s.as_bytes()).unwrap_or(b"");
            if self.current_chunk.is_empty() {
                return b"";
            }
        }

        let offset_in_chunk = byte_offset.saturating_sub(self.total_byte_offset);
        &self.current_chunk[offset_in_chunk.min(self.current_chunk.len())..]
    }
}

/// Matches Zed's heuristic.
pub const MAX_BYTES_TO_QUERY_INTERNAL: usize = 16 * 1024;

/// Compiled highlight query + reusable cursor.
pub struct TreeSitterProvider {
    query: Option<Query>,
    capture_names: Vec<Arc<str>>,
    query_cursor: QueryCursor,
}

impl TreeSitterProvider {
    pub fn new() -> Self {
        Self {
            query: None,
            capture_names: Vec::new(),
            query_cursor: QueryCursor::new(),
        }
    }

    pub fn set_query(
        &mut self,
        query_source: &str,
        language: Language,
    ) -> Result<(), crate::ts::QueryError> {
        let query = Query::new(&language, query_source)?;
        self.capture_names = query
            .capture_names()
            .iter()
            .map(|name| Arc::<str>::from(*name))
            .collect();
        self.query = Some(query);
        Ok(())
    }

    pub fn is_available(&self) -> bool {
        self.query.is_some()
    }

    pub fn query(&self) -> Option<&Query> {
        self.query.as_ref()
    }

    pub fn capture_names(&self) -> &[Arc<str>] {
        &self.capture_names
    }

    pub fn query_cursor_mut(&mut self) -> &mut QueryCursor {
        &mut self.query_cursor
    }

    pub fn highlight_range(
        &mut self,
        tree: &Tree,
        rope: &Rope,
        byte_range: Range<usize>,
    ) -> Option<Vec<HighlightRange>> {
        let query = self.query.as_ref()?;
        let query_end =
            byte_range.start + (byte_range.end - byte_range.start).min(MAX_BYTES_TO_QUERY_INTERNAL);
        Some(highlight_ranges(
            tree,
            rope,
            query,
            &self.capture_names,
            &mut self.query_cursor,
            byte_range.start..query_end,
        ))
    }
}

impl Default for TreeSitterProvider {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn build_parser(language: &Language) -> Option<Parser> {
    let mut parser = Parser::new();
    parser.set_language(language).ok()?;
    Some(parser)
}
