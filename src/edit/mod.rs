pub mod nodes;
pub mod undo;

use nodes::{EditNode, EditOp};
use undo::UndoStack;

pub struct EditStack {
    pub nodes: Vec<EditNode>,
    undo: UndoStack,
    next_id: u64,
}

impl Default for EditStack {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            undo: UndoStack::default(),
            next_id: 1,
        }
    }
}

impl EditStack {
    pub fn add(&mut self, op: EditOp) {
        self.undo.push(self.nodes.clone());
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.push(EditNode::new(id, op));
    }

    pub fn remove(&mut self, id: u64) {
        self.undo.push(self.nodes.clone());
        self.nodes.retain(|n| n.id != id);
    }

    pub fn toggle(&mut self, id: u64) {
        self.undo.push(self.nodes.clone());
        if let Some(n) = self.nodes.iter_mut().find(|n| n.id == id) {
            n.enabled = !n.enabled;
        }
    }

    pub fn update(&mut self, id: u64, op: EditOp) {
        self.undo.push(self.nodes.clone());
        if let Some(n) = self.nodes.iter_mut().find(|n| n.id == id) {
            n.op = op;
        }
    }

    pub fn move_node(&mut self, from: usize, to: usize) {
        if from == to || from >= self.nodes.len() || to >= self.nodes.len() {
            return;
        }
        self.undo.push(self.nodes.clone());
        let node = self.nodes.remove(from);
        self.nodes.insert(to, node);
    }

    pub fn undo(&mut self) {
        if let Some(prev) = self.undo.undo(self.nodes.clone()) {
            self.nodes = prev;
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.undo.redo(self.nodes.clone()) {
            self.nodes = next;
        }
    }

    pub fn can_undo(&self) -> bool {
        self.undo.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.undo.can_redo()
    }
}
