//! `SetLanguageRequested` handler — swaps the editor's
//! `bevy_tree_sitter::TreeSitterGrammar` component. The downstream parser
//! observes the change and re-parses on the next frame.

use bevy::prelude::*;

use crate::types::events::SetLanguageRequested;
use crate::types::CodeEditor;

pub fn handle_set_language(
    mut commands: Commands,
    mut events: MessageReader<SetLanguageRequested>,
    editors: Query<Entity, With<CodeEditor>>,
) {
    for ev in events.read() {
        if editors.get(ev.entity).is_err() {
            continue;
        }
        let mut entity = commands.entity(ev.entity);
        match &ev.grammar {
            Some(grammar) => {
                entity.insert(grammar.clone());
            }
            None => {
                entity.remove::<bevy_tree_sitter::TreeSitterGrammar>();
            }
        }
    }
}
