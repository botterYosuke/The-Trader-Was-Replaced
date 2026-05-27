use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;
use leafwing_input_manager::prelude::*;

pub fn default_input_map() -> InputMap<EditorAction> {
    let mut input_map = InputMap::default();

    input_map.insert(EditorAction::DeleteBackward, KeyCode::Backspace);
    input_map.insert(EditorAction::DeleteForward, KeyCode::Delete);
    input_map.insert(
        EditorAction::DeleteWordBackward,
        ButtonlikeChord::new([KeyCode::ControlLeft, KeyCode::Backspace]),
    );
    input_map.insert(
        EditorAction::DeleteWordForward,
        ButtonlikeChord::new([KeyCode::ControlLeft, KeyCode::Delete]),
    );

    input_map.insert(EditorAction::InsertNewline, KeyCode::Enter);
    input_map.insert(EditorAction::InsertTab, KeyCode::Tab);

    input_map.insert(EditorAction::MoveCursorLeft, KeyCode::ArrowLeft);
    input_map.insert(EditorAction::MoveCursorRight, KeyCode::ArrowRight);
    input_map.insert(EditorAction::MoveCursorUp, KeyCode::ArrowUp);
    input_map.insert(EditorAction::MoveCursorDown, KeyCode::ArrowDown);
    input_map.insert(
        EditorAction::MoveCursorWordLeft,
        ButtonlikeChord::new([KeyCode::ControlLeft, KeyCode::ArrowLeft]),
    );
    input_map.insert(
        EditorAction::MoveCursorWordRight,
        ButtonlikeChord::new([KeyCode::ControlLeft, KeyCode::ArrowRight]),
    );
    input_map.insert(EditorAction::MoveCursorLineStart, KeyCode::Home);
    input_map.insert(EditorAction::MoveCursorLineEnd, KeyCode::End);
    input_map.insert(
        EditorAction::MoveCursorDocumentStart,
        ButtonlikeChord::new([KeyCode::ControlLeft, KeyCode::Home]),
    );
    input_map.insert(
        EditorAction::MoveCursorDocumentEnd,
        ButtonlikeChord::new([KeyCode::ControlLeft, KeyCode::End]),
    );
    input_map.insert(EditorAction::MoveCursorPageUp, KeyCode::PageUp);
    input_map.insert(EditorAction::MoveCursorPageDown, KeyCode::PageDown);

    input_map.insert(
        EditorAction::SelectLeft,
        ButtonlikeChord::new([KeyCode::ShiftLeft, KeyCode::ArrowLeft]),
    );
    input_map.insert(
        EditorAction::SelectRight,
        ButtonlikeChord::new([KeyCode::ShiftLeft, KeyCode::ArrowRight]),
    );
    input_map.insert(
        EditorAction::SelectUp,
        ButtonlikeChord::new([KeyCode::ShiftLeft, KeyCode::ArrowUp]),
    );
    input_map.insert(
        EditorAction::SelectDown,
        ButtonlikeChord::new([KeyCode::ShiftLeft, KeyCode::ArrowDown]),
    );
    input_map.insert(
        EditorAction::SelectWordLeft,
        ButtonlikeChord::new([KeyCode::ControlLeft, KeyCode::ShiftLeft, KeyCode::ArrowLeft]),
    );
    input_map.insert(
        EditorAction::SelectWordRight,
        ButtonlikeChord::new([
            KeyCode::ControlLeft,
            KeyCode::ShiftLeft,
            KeyCode::ArrowRight,
        ]),
    );
    input_map.insert(
        EditorAction::SelectLineStart,
        ButtonlikeChord::new([KeyCode::ShiftLeft, KeyCode::Home]),
    );
    input_map.insert(
        EditorAction::SelectLineEnd,
        ButtonlikeChord::new([KeyCode::ShiftLeft, KeyCode::End]),
    );
    input_map.insert(
        EditorAction::SelectAll,
        ButtonlikeChord::new([KeyCode::ControlLeft, KeyCode::KeyA]),
    );
    input_map.insert(EditorAction::ClearSelection, KeyCode::Escape);

    input_map.insert(
        EditorAction::Copy,
        ButtonlikeChord::new([KeyCode::ControlLeft, KeyCode::KeyC]),
    );
    input_map.insert(
        EditorAction::Cut,
        ButtonlikeChord::new([KeyCode::ControlLeft, KeyCode::KeyX]),
    );
    input_map.insert(
        EditorAction::Paste,
        ButtonlikeChord::new([KeyCode::ControlLeft, KeyCode::KeyV]),
    );

    input_map.insert(
        EditorAction::Undo,
        ButtonlikeChord::new([KeyCode::ControlLeft, KeyCode::KeyZ]),
    );
    input_map.insert(
        EditorAction::Redo,
        ButtonlikeChord::new([KeyCode::ControlLeft, KeyCode::KeyY]),
    );
    input_map.insert(
        EditorAction::Redo,
        ButtonlikeChord::new([KeyCode::ControlLeft, KeyCode::ShiftLeft, KeyCode::KeyZ]),
    );

    input_map.insert(
        EditorAction::GotoLine,
        ButtonlikeChord::new([KeyCode::ControlLeft, KeyCode::KeyG]),
    );

    input_map.insert(
        EditorAction::RequestCompletion,
        ButtonlikeChord::new([KeyCode::ControlLeft, KeyCode::Space]),
    );
    input_map.insert(EditorAction::RenameSymbol, KeyCode::F2);

    input_map.insert(
        EditorAction::AddCursorAtNextOccurrence,
        ButtonlikeChord::new([KeyCode::ControlLeft, KeyCode::KeyD]),
    );
    input_map.insert(
        EditorAction::AddCursorAbove,
        ButtonlikeChord::new([KeyCode::ControlLeft, KeyCode::AltLeft, KeyCode::ArrowUp]),
    );
    input_map.insert(
        EditorAction::AddCursorBelow,
        ButtonlikeChord::new([KeyCode::ControlLeft, KeyCode::AltLeft, KeyCode::ArrowDown]),
    );

    input_map.insert(
        EditorAction::ToggleFold,
        ButtonlikeChord::new([
            KeyCode::ControlLeft,
            KeyCode::ShiftLeft,
            KeyCode::BracketLeft,
        ]),
    );
    input_map.insert(
        EditorAction::Fold,
        ButtonlikeChord::new([
            KeyCode::ControlLeft,
            KeyCode::ShiftLeft,
            KeyCode::BracketLeft,
        ]),
    );
    input_map.insert(
        EditorAction::Unfold,
        ButtonlikeChord::new([
            KeyCode::ControlLeft,
            KeyCode::ShiftLeft,
            KeyCode::BracketRight,
        ]),
    );
    input_map.insert(
        EditorAction::FoldAll,
        ButtonlikeChord::new([KeyCode::ControlLeft, KeyCode::AltLeft, KeyCode::BracketLeft]),
    );
    input_map.insert(
        EditorAction::UnfoldAll,
        ButtonlikeChord::new([
            KeyCode::ControlLeft,
            KeyCode::AltLeft,
            KeyCode::BracketRight,
        ]),
    );

    input_map.insert(
        EditorAction::Save,
        ButtonlikeChord::new([KeyCode::ControlLeft, KeyCode::KeyS]),
    );
    input_map.insert(
        EditorAction::Open,
        ButtonlikeChord::new([KeyCode::ControlLeft, KeyCode::KeyO]),
    );

    input_map
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect, Actionlike)]
#[reflect(Debug, Hash, PartialEq)]
pub enum EditorAction {
    DeleteBackward,
    DeleteForward,
    DeleteWordBackward,
    DeleteWordForward,
    DeleteLine,

    InsertNewline,
    InsertTab,

    MoveCursorLeft,
    MoveCursorRight,
    MoveCursorUp,
    MoveCursorDown,
    MoveCursorWordLeft,
    MoveCursorWordRight,
    MoveCursorLineStart,
    MoveCursorLineEnd,
    MoveCursorDocumentStart,
    MoveCursorDocumentEnd,
    MoveCursorPageUp,
    MoveCursorPageDown,

    SelectLeft,
    SelectRight,
    SelectUp,
    SelectDown,
    SelectWordLeft,
    SelectWordRight,
    SelectLineStart,
    SelectLineEnd,
    SelectAll,
    ClearSelection,

    Copy,
    Cut,
    Paste,

    Undo,
    Redo,

    GotoLine,

    RequestCompletion,
    GotoDefinition,
    RenameSymbol,

    AddCursorAtNextOccurrence,
    AddCursorAbove,
    AddCursorBelow,
    ClearSecondaryCursors,

    ToggleFold,
    Fold,
    Unfold,
    FoldAll,
    UnfoldAll,

    Save,
    Open,
}

impl EditorAction {
    pub fn is_repeatable(&self) -> bool {
        matches!(
            self,
            EditorAction::DeleteBackward
                | EditorAction::DeleteForward
                | EditorAction::DeleteWordBackward
                | EditorAction::DeleteWordForward
                | EditorAction::MoveCursorLeft
                | EditorAction::MoveCursorRight
                | EditorAction::MoveCursorUp
                | EditorAction::MoveCursorDown
                | EditorAction::MoveCursorWordLeft
                | EditorAction::MoveCursorWordRight
                | EditorAction::SelectLeft
                | EditorAction::SelectRight
                | EditorAction::SelectUp
                | EditorAction::SelectDown
                | EditorAction::SelectWordLeft
                | EditorAction::SelectWordRight
                | EditorAction::Undo
                | EditorAction::Redo
                | EditorAction::InsertNewline
        )
    }

    /// Actions that mutate buffer contents. Suppressed when `Misc::read_only`
    /// is set; navigation, selection, copy, search, and fold actions stay
    /// available so the editor still behaves like a usable viewer.
    pub fn is_mutating(&self) -> bool {
        matches!(
            self,
            EditorAction::DeleteBackward
                | EditorAction::DeleteForward
                | EditorAction::DeleteWordBackward
                | EditorAction::DeleteWordForward
                | EditorAction::DeleteLine
                | EditorAction::InsertNewline
                | EditorAction::InsertTab
                | EditorAction::Cut
                | EditorAction::Paste
                | EditorAction::Undo
                | EditorAction::Redo
                | EditorAction::RenameSymbol
        )
    }
}
