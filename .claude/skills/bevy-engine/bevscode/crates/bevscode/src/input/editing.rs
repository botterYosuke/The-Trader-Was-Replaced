//! Editor-side observers that react to [`bevy_instanced_text_editor::OnEdit`] triggers.

use crate::types::events::TextEdited;
use bevy::prelude::*;
use bevy_instanced_text::TextBuffer;
use bevy_instanced_text_editor::{OnEdit, RopeBuffer};

pub fn on_edit_invalidate_caches(
    trigger: On<OnEdit>,
    q: Query<&TextBuffer<RopeBuffer>, With<crate::types::CodeEditor>>,
    mut writer: MessageWriter<TextEdited>,
    mut version: Local<u64>,
) {
    let entity = trigger.event().entity;
    let Ok(_buffer) = q.get(entity) else {
        return;
    };

    if let Some(byte_edit) = trigger.event().byte_edit {
        *version = version.wrapping_add(1);
        writer.write(
            TextEdited::new(byte_edit, *version)
                .with_pre_edit_rope(trigger.event().pre_edit_rope.clone()),
        );
    }
}
