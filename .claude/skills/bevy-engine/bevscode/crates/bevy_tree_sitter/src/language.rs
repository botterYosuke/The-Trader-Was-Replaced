use crate::tree_sitter::TreeSitterProvider;
use crate::ts;
use bevy_ecs::prelude::*;

/// Not Reflect: `ts::Language` owns FFI-side state.
#[derive(Component, Clone)]
pub struct TreeSitterGrammar {
    pub grammar: ts::Language,
    pub highlights_query: String,
}

impl TreeSitterGrammar {
    pub fn new(grammar: ts::Language, highlights_query: impl Into<String>) -> Self {
        Self {
            grammar,
            highlights_query: highlights_query.into(),
        }
    }

    /// Returns `None` if the highlight query fails to compile.
    pub fn create_provider(&self) -> Option<TreeSitterProvider> {
        let mut provider = TreeSitterProvider::new();
        provider
            .set_query(&self.highlights_query, self.grammar.clone())
            .ok()?;
        Some(provider)
    }
}
