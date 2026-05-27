//! Typed editing-request events consumed by per-action handler systems.

use bevy::prelude::*;

macro_rules! editing_event {
    ($($name:ident),* $(,)?) => {
        $(
            #[derive(Message, Clone, Copy, Debug, Default, Reflect)]
            #[reflect(Clone, Debug, Default)]
            pub struct $name;
        )*
    };
}

editing_event!(
    MoveCursorLeftRequested,
    MoveCursorRightRequested,
    MoveCursorUpRequested,
    MoveCursorDownRequested,
    MoveCursorWordLeftRequested,
    MoveCursorWordRightRequested,
    MoveCursorLineStartRequested,
    MoveCursorLineEndRequested,
    MoveCursorDocumentStartRequested,
    MoveCursorDocumentEndRequested,
    MoveCursorPageUpRequested,
    MoveCursorPageDownRequested,
);

editing_event!(
    SelectLeftRequested,
    SelectRightRequested,
    SelectUpRequested,
    SelectDownRequested,
    SelectWordLeftRequested,
    SelectWordRightRequested,
    SelectLineStartRequested,
    SelectLineEndRequested,
    SelectAllRequested,
    ClearSelectionRequested,
);

editing_event!(
    DeleteBackwardRequested,
    DeleteForwardRequested,
    DeleteWordBackwardRequested,
    DeleteWordForwardRequested,
    DeleteLineRequested,
    InsertNewlineRequested,
    InsertTabRequested,
);

editing_event!(CopyRequested, CutRequested, PasteRequested);

editing_event!(UndoRequested, RedoRequested);

#[derive(Message, Clone, Debug, Reflect)]
#[reflect(Clone, Debug)]
pub struct ReplaceRangeRequested {
    pub entity: Entity,
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub kind: crate::history::EditKind,
    pub record_history: bool,
}

#[derive(Message, Clone, Debug, Reflect)]
#[reflect(Clone, Debug)]
pub struct SetTextRequested {
    pub entity: Entity,
    pub text: String,
}

/// Emitted at most once per editor per frame. `from`/`to` are rope char offsets.
#[derive(Message, Clone, Copy, Debug, Reflect)]
#[reflect(Clone, Debug)]
pub struct CursorMoved {
    pub entity: Entity,
    pub from: usize,
    pub to: usize,
}

/// Emitted at most once per editor per frame when selections change.
#[derive(Message, Clone, Debug, Reflect)]
#[reflect(Clone, Debug)]
pub struct SelectionChanged {
    pub entity: Entity,
    pub selection_count: usize,
    pub total_chars_selected: usize,
}
