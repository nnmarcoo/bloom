use super::nodes::EditNode;

const MAX_UNDO: usize = 100;

/// Snapshot-based undo stack. Each entry is a full clone of the edit stack.
/// This is cheap since node params are small value types.
pub struct UndoStack {
    past: Vec<Vec<EditNode>>,
    future: Vec<Vec<EditNode>>,
}

impl Default for UndoStack {
    fn default() -> Self {
        Self { past: Vec::new(), future: Vec::new() }
    }
}

impl UndoStack {
    /// Call before any mutating operation on the edit stack.
    pub fn push(&mut self, snapshot: Vec<EditNode>) {
        if self.past.len() >= MAX_UNDO {
            self.past.remove(0);
        }
        self.past.push(snapshot);
        self.future.clear();
    }

    /// Returns the previous state if available.
    pub fn undo(&mut self, current: Vec<EditNode>) -> Option<Vec<EditNode>> {
        let prev = self.past.pop()?;
        self.future.push(current);
        Some(prev)
    }

    /// Returns the next state if available.
    pub fn redo(&mut self, current: Vec<EditNode>) -> Option<Vec<EditNode>> {
        let next = self.future.pop()?;
        self.past.push(current);
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }
}
