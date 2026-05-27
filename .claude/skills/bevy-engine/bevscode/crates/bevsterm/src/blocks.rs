//! OSC 133 → [`TerminalBlockState`].

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use bevy::prelude::*;

use crate::backend::{SemanticType, SemanticZone};
use crate::messages::TerminalBlockFinished;
use crate::text::{BlockStatus, TerminalBlock, TerminalBlockState, TerminalSession};

pub fn extract_blocks(
    mut q: Query<(Entity, &TerminalSession, &mut TerminalBlockState)>,
    mut finished_w: MessageWriter<TerminalBlockFinished>,
) {
    for (entity, session, mut state) in q.iter_mut() {
        let zones = session
            .terminal
            .lock()
            .get_semantic_zones()
            .unwrap_or_default();
        let new_blocks = group_zones_into_blocks(&zones);

        for nb in &new_blocks {
            if nb.status != BlockStatus::Completed {
                continue;
            }
            let was_completed = state
                .blocks
                .iter()
                .any(|ob| ob.id == nb.id && ob.status == BlockStatus::Completed);
            if !was_completed {
                finished_w.write(TerminalBlockFinished {
                    entity,
                    block_id: nb.id,
                    exit_code: nb.exit_code,
                });
            }
        }

        let current = new_blocks
            .iter()
            .rposition(|b| b.status == BlockStatus::Running);
        state.blocks = new_blocks;
        state.current_block = current;
    }
}

pub fn group_zones_into_blocks(zones: &[SemanticZone]) -> Vec<TerminalBlock> {
    let mut out = Vec::<TerminalBlock>::new();
    let mut current: Option<TerminalBlock> = None;

    for z in zones {
        match z.semantic_type {
            SemanticType::Prompt => {
                if let Some(b) = current.take() {
                    out.push(b);
                }
                current = Some(TerminalBlock {
                    id: 0,
                    status: BlockStatus::Running,
                    exit_code: None,
                    prompt_row: z.start_y as i64,
                    output_row: z.end_y as i64,
                    end_row: z.end_y as i64,
                    command_text: String::new(),
                });
            }
            SemanticType::Input => {
                let block = current.get_or_insert_with(|| TerminalBlock {
                    id: 0,
                    status: BlockStatus::Running,
                    exit_code: None,
                    prompt_row: z.start_y as i64,
                    output_row: z.start_y as i64,
                    end_row: z.end_y as i64,
                    command_text: String::new(),
                });
                block.output_row = z.end_y as i64;
                block.end_row = z.end_y as i64;
            }
            SemanticType::Output => {
                let block = current.get_or_insert_with(|| TerminalBlock {
                    id: 0,
                    status: BlockStatus::Running,
                    exit_code: None,
                    prompt_row: z.start_y as i64,
                    output_row: z.start_y as i64,
                    end_row: z.end_y as i64,
                    command_text: String::new(),
                });
                block.end_row = z.end_y as i64;
            }
        }
    }
    if let Some(b) = current.take() {
        out.push(b);
    }

    let len = out.len();
    for (i, block) in out.iter_mut().enumerate() {
        if i + 1 < len {
            block.status = BlockStatus::Completed;
        }
    }

    for block in out.iter_mut() {
        let mut h = DefaultHasher::new();
        block.prompt_row.hash(&mut h);
        block.command_text.hash(&mut h);
        block.id = h.finish();
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(kind: SemanticType, start_y: i64, end_y: i64) -> SemanticZone {
        SemanticZone {
            start_y: start_y as isize,
            start_x: 0,
            end_y: end_y as isize,
            end_x: 0,
            semantic_type: kind,
        }
    }

    #[test]
    fn empty_zones_produce_no_blocks() {
        assert!(group_zones_into_blocks(&[]).is_empty());
    }

    #[test]
    fn single_running_block() {
        let zones = vec![
            zone(SemanticType::Prompt, 0, 0),
            zone(SemanticType::Input, 0, 0),
            zone(SemanticType::Output, 1, 3),
        ];
        let blocks = group_zones_into_blocks(&zones);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].status, BlockStatus::Running);
        assert_eq!(blocks[0].prompt_row, 0);
        assert_eq!(blocks[0].end_row, 3);
    }

    #[test]
    fn two_blocks_first_completed_second_running() {
        let zones = vec![
            zone(SemanticType::Prompt, 0, 0),
            zone(SemanticType::Output, 1, 2),
            zone(SemanticType::Prompt, 3, 3),
            zone(SemanticType::Output, 4, 5),
        ];
        let blocks = group_zones_into_blocks(&zones);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].status, BlockStatus::Completed);
        assert_eq!(blocks[0].prompt_row, 0);
        assert_eq!(blocks[0].end_row, 2);
        assert_eq!(blocks[1].status, BlockStatus::Running);
        assert_eq!(blocks[1].prompt_row, 3);
        assert_eq!(blocks[1].end_row, 5);
        assert_ne!(blocks[0].id, blocks[1].id);
    }

    #[test]
    fn block_id_stable_across_calls() {
        let zones = vec![
            zone(SemanticType::Prompt, 5, 5),
            zone(SemanticType::Output, 6, 8),
            zone(SemanticType::Prompt, 9, 9),
        ];
        let a = group_zones_into_blocks(&zones);
        let b = group_zones_into_blocks(&zones);
        assert_eq!(a[0].id, b[0].id);
        assert_eq!(a[1].id, b[1].id);
    }
}
