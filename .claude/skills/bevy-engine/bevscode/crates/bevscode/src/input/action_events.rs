//! Typed events emitted by `dispatch_action_events` -- one per
//! [`super::EditorAction`] variant.

use bevy::prelude::*;

pub use bevy_instanced_text_editor::{
    ClearSelectionRequested, CopyRequested, CutRequested, DeleteBackwardRequested,
    DeleteForwardRequested, DeleteLineRequested, DeleteWordBackwardRequested,
    DeleteWordForwardRequested, InsertNewlineRequested, InsertTabRequested,
    MoveCursorDocumentEndRequested, MoveCursorDocumentStartRequested, MoveCursorDownRequested,
    MoveCursorLeftRequested, MoveCursorLineEndRequested, MoveCursorLineStartRequested,
    MoveCursorPageDownRequested, MoveCursorPageUpRequested, MoveCursorRightRequested,
    MoveCursorUpRequested, MoveCursorWordLeftRequested, MoveCursorWordRightRequested,
    PasteRequested, RedoRequested, SelectAllRequested, SelectDownRequested, SelectLeftRequested,
    SelectLineEndRequested, SelectLineStartRequested, SelectRightRequested, SelectUpRequested,
    SelectWordLeftRequested, SelectWordRightRequested, UndoRequested,
};

macro_rules! action_event {
    ($($name:ident),* $(,)?) => {
        $(
            #[derive(Message, Clone, Copy, Debug, Default, Reflect)]
            #[reflect(Clone, Debug, Default)]
            pub struct $name;
        )*
    };
}

action_event!(GotoLineRequested);

action_event!(
    RequestCompletionRequested,
    GotoDefinitionRequested,
    RenameSymbolRequested,
);

action_event!(
    AddCursorAtNextOccurrenceRequested,
    AddCursorAboveRequested,
    AddCursorBelowRequested,
    ClearSecondaryCursorsRequested,
);

action_event!(
    ToggleFoldRequested,
    FoldRequested,
    UnfoldRequested,
    FoldAllRequested,
    UnfoldAllRequested,
);

