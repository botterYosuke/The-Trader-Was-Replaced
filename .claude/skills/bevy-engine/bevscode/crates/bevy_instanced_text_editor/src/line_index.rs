//! Helpers for tracking buffer-line indices across edits.

use crate::text_state::EditDelta;

/// What happened to a single buffer line across one edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineShift {
    Unchanged,
    Moved(u32),
    Deleted,
}

pub fn shift_line(row: u32, delta: &EditDelta) -> LineShift {
    let start_row = delta.start_position.row;
    let old_end_row = delta.old_end_position.row;
    let new_end_row = delta.new_end_position.row;

    if old_end_row == new_end_row {
        return LineShift::Unchanged;
    }

    if new_end_row > old_end_row {
        let shift = (new_end_row - old_end_row) as i64;
        if row > old_end_row {
            return LineShift::Moved(((row as i64) + shift) as u32);
        }
        return LineShift::Unchanged;
    }

    // Deletion: rows fully inside (start_row, old_end_row] vanished; rows
    // past old_end_row slide up by the line-count delta.
    let shift = (old_end_row - new_end_row) as i64;
    if row <= start_row {
        return LineShift::Unchanged;
    }
    if row <= old_end_row {
        return LineShift::Deleted;
    }
    LineShift::Moved(((row as i64) - shift).max(0) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text_state::EditPoint;

    fn delta(start: u32, old_end: u32, new_end: u32) -> EditDelta {
        EditDelta {
            start_byte: 0,
            old_end_byte: 0,
            new_end_byte: 0,
            start_position: EditPoint {
                row: start,
                column_byte: 0,
            },
            old_end_position: EditPoint {
                row: old_end,
                column_byte: 0,
            },
            new_end_position: EditPoint {
                row: new_end,
                column_byte: 0,
            },
        }
    }

    /// Insertion of one newline at row 3 (split): rows 0..=3 stay; row 4+
    /// shift up by one.
    #[test]
    fn insert_newline_shifts_trailing_rows() {
        let d = delta(3, 3, 4);
        assert_eq!(shift_line(0, &d), LineShift::Unchanged);
        assert_eq!(shift_line(3, &d), LineShift::Unchanged);
        assert_eq!(shift_line(4, &d), LineShift::Moved(5));
        assert_eq!(shift_line(10, &d), LineShift::Moved(11));
    }

    /// Backspace-join at row 4 (removed the `\n` ending row 3): row 4 is
    /// deleted; rows 5+ slide down by one.
    #[test]
    fn delete_newline_drops_one_row() {
        let d = delta(3, 4, 3);
        assert_eq!(shift_line(0, &d), LineShift::Unchanged);
        assert_eq!(shift_line(3, &d), LineShift::Unchanged);
        assert_eq!(shift_line(4, &d), LineShift::Deleted);
        assert_eq!(shift_line(5, &d), LineShift::Moved(4));
        assert_eq!(shift_line(10, &d), LineShift::Moved(9));
    }

    /// Multi-line delete (rows 2..=4 removed): rows 3,4 vanish; row 5+
    /// slide down by 2.
    #[test]
    fn multi_line_delete() {
        let d = delta(2, 4, 2);
        assert_eq!(shift_line(0, &d), LineShift::Unchanged);
        assert_eq!(shift_line(2, &d), LineShift::Unchanged);
        assert_eq!(shift_line(3, &d), LineShift::Deleted);
        assert_eq!(shift_line(4, &d), LineShift::Deleted);
        assert_eq!(shift_line(5, &d), LineShift::Moved(3));
    }

    /// Same-row edit (typing a character): no row movement at all.
    #[test]
    fn same_row_edit_is_noop() {
        let d = delta(2, 2, 2);
        for row in 0..10 {
            assert_eq!(shift_line(row, &d), LineShift::Unchanged);
        }
    }
}
