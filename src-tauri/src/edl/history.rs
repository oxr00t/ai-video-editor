//! Snapshot-based undo/redo. Projects are small JSON documents, so whole-state
//! snapshots keep every operation (human drag, agent tool call, recipe) under
//! one uniform undo model.

use super::Project;

pub struct History {
    undo: Vec<Project>,
    redo: Vec<Project>,
    limit: usize,
}

impl History {
    pub fn new() -> Self {
        History {
            undo: Vec::new(),
            redo: Vec::new(),
            limit: 100,
        }
    }

    pub fn push(&mut self, snapshot: Project) {
        self.redo.clear();
        self.undo.push(snapshot);
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
    }

    /// Restore the last snapshot without touching the redo stack — used when a
    /// mutation fails validation and must be rolled back.
    pub fn rollback(&mut self, current: &mut Project) -> bool {
        match self.undo.pop() {
            Some(prev) => {
                *current = prev;
                true
            }
            None => false,
        }
    }

    pub fn undo(&mut self, current: &mut Project) -> bool {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(current.clone());
            *current = prev;
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self, current: &mut Project) -> bool {
        if let Some(next) = self.redo.pop() {
            self.undo.push(current.clone());
            *current = next;
            true
        } else {
            false
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}
