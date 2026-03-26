use std::collections::HashMap;

use log::warn;


#[derive(Debug)]
pub struct NtfsNode {
    pub name: String,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub mft_record: usize,
}

#[derive(Debug)]
pub struct Arena {
    pub nodes: Vec<NtfsNode>
}

impl Arena {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
        }
    }

    fn add_node(&mut self, name: &str, mft_record: usize) -> usize {
        let node_id = self.nodes.len();
        self.nodes.push(NtfsNode {
            name: name.to_string(),
            parent: None,
            children: Vec::new(),
            mft_record
        });
        node_id
    }

    fn link_parent_child(&mut self, parent_id: usize, child_id: usize) {
        if self.is_descendant(parent_id, child_id) {
            warn!(
                "Cannot link parent {} to child {}: would create a cycle",
                parent_id, child_id
            );
            return;
        }

        self.nodes[child_id].parent = Some(parent_id);
        self.nodes[parent_id].children.push(child_id);
    }

    fn is_descendant(&self, potential_ancestor: usize, node_id: usize) -> bool {
        let mut stack = vec![potential_ancestor];

        while let Some(current) = stack.pop() {
            if current == node_id {
                return true; // cycle detected
            }
            stack.extend(&self.nodes[current].children);
        }

        false
    }
}

#[derive(Debug)]
pub struct NtfsTree {
    pub arena: Arena,
    mft_to_node: HashMap<usize, usize>,
    pending_children: HashMap<usize, Vec<usize>>,
    root: Option<usize>,
}

impl NtfsTree {
    pub fn new() -> Self {
        Self {
            arena: Arena::new(),
            mft_to_node: HashMap::new(),
            pending_children: HashMap::new(),
            root: None,
        }
    }

    pub fn add_entry(&mut self, name: &str, mft_record: usize, parent_mft: usize) {
        let node_id = self.arena.add_node(name, mft_record);
        self.mft_to_node.insert(mft_record, node_id);

        if parent_mft == mft_record {
            self.root = Some(node_id);
        }

        if let Some(&parent_id) = self.mft_to_node.get(&parent_mft) {
            self.arena.link_parent_child(parent_id, node_id);
        } else {
            self.pending_children
                .entry(parent_mft)
                .or_default()
                .push(node_id);
        }

        if let Some(children) = self.pending_children.remove(&mft_record) {
            for child in children {
                self.arena.link_parent_child(node_id, child);
            }
        }
    }

    pub fn get(&self, node_id: usize) -> &NtfsNode {
        &self.arena.nodes[node_id]
    }

    pub fn get_ordered_nodes(&self) -> Vec<usize> {
        let mut result = Vec::new();

        if let Some(root_id) = self.root {
            self.collect_nodes_recursive(root_id, &mut result);
        }
        result
    }

    fn collect_nodes_recursive(&self, parent_id: usize, result: &mut Vec<usize>) {
        result.push(parent_id);
        for child in &self.arena.nodes[parent_id].children {
            self.collect_nodes_recursive(*child, result);
        }
    }
}