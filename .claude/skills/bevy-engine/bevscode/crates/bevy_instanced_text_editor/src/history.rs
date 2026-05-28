use bevy::platform::time::Instant;
use bevy::reflect::Reflect;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Reflect)]
pub enum EditKind {
    Insert,
    DeleteBackward,
    DeleteForward,
    Newline,
    Paste,
    Other,
}

#[derive(Clone, Debug)]
pub struct EditOperation {
    pub removed_text: String,
    pub inserted_text: String,
    /// Char index where the edit occurred.
    pub position: usize,
    pub cursor_before: usize,
    pub cursor_after: usize,
    pub kind: EditKind,
}

/// Groups multiple edits that should be undone/redone together.
#[derive(Clone, Debug)]
pub struct EditTransaction {
    pub operations: Vec<EditOperation>,
    pub timestamp: Instant,
}

impl EditTransaction {
    pub fn new() -> Self {
        Self {
            operations: Vec::new(),
            timestamp: Instant::now(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }
}

impl Default for EditTransaction {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub struct EditHistory {
    pub undo_stack: Vec<EditTransaction>,
    pub redo_stack: Vec<EditTransaction>,
    /// Accumulates rapid edits into a single undoable unit.
    pub current_transaction: Option<EditTransaction>,
    pub group_interval_ms: u64,
    pub max_history_size: usize,
}

impl Default for EditHistory {
    fn default() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            current_transaction: None,
            group_interval_ms: 300,
            max_history_size: 1000,
        }
    }
}

impl EditHistory {
    pub fn record(&mut self, operation: EditOperation) {
        let now = Instant::now();
        let op_kind = operation.kind;

        let should_start_new = match &self.current_transaction {
            Some(tx) => {
                let elapsed = now.duration_since(tx.timestamp).as_millis() as u64;

                if elapsed > self.group_interval_ms {
                    return self.start_new_transaction(operation, now);
                }

                if matches!(
                    op_kind,
                    EditKind::Newline | EditKind::Paste | EditKind::Other
                ) {
                    return self.start_new_transaction(operation, now);
                }

                if let Some(last_op) = tx.operations.last() {
                    let last_kind = last_op.kind;
                    let kind_changed = !matches!(
                        (last_kind, op_kind),
                        (EditKind::Insert, EditKind::Insert)
                            | (EditKind::DeleteBackward, EditKind::DeleteBackward)
                            | (EditKind::DeleteForward, EditKind::DeleteForward)
                    );

                    if kind_changed {
                        return self.start_new_transaction(operation, now);
                    }

                    let is_contiguous = match op_kind {
                        EditKind::Insert => operation.position == last_op.cursor_after,
                        EditKind::DeleteBackward => operation.cursor_before == last_op.cursor_after,
                        EditKind::DeleteForward => operation.position == last_op.position,
                        _ => false,
                    };

                    if !is_contiguous {
                        return self.start_new_transaction(operation, now);
                    }
                }

                false
            }
            None => true,
        };

        if should_start_new {
            self.start_new_transaction(operation, now);
        } else {
            if let Some(tx) = &mut self.current_transaction {
                tx.operations.push(operation);
                tx.timestamp = now;
            }
        }

        self.redo_stack.clear();
    }

    fn start_new_transaction(&mut self, operation: EditOperation, timestamp: Instant) {
        self.finalize_transaction();
        self.current_transaction = Some(EditTransaction {
            operations: vec![operation],
            timestamp,
        });
        self.redo_stack.clear();
    }

    pub fn finalize_transaction(&mut self) {
        if let Some(tx) = self.current_transaction.take() {
            if !tx.is_empty() {
                self.undo_stack.push(tx);
                if self.undo_stack.len() > self.max_history_size {
                    let excess = self.undo_stack.len() - self.max_history_size;
                    self.undo_stack.drain(..excess);
                }
            }
        }
    }

    pub fn pop_undo(&mut self) -> Option<EditTransaction> {
        self.finalize_transaction();
        self.undo_stack.pop()
    }

    pub fn push_redo(&mut self, transaction: EditTransaction) {
        self.redo_stack.push(transaction);
    }

    pub fn pop_redo(&mut self) -> Option<EditTransaction> {
        self.redo_stack.pop()
    }

    pub fn push_undo(&mut self, transaction: EditTransaction) {
        self.undo_stack.push(transaction);
        if self.undo_stack.len() > self.max_history_size {
            let excess = self.undo_stack.len() - self.max_history_size;
            self.undo_stack.drain(..excess);
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
            || self
                .current_transaction
                .as_ref()
                .is_some_and(|tx| !tx.is_empty())
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.current_transaction = None;
    }
}
