
#[derive(Debug)]
struct NtfsNode {
    name: String,
    parent: Option<usize>,
    children: Vec<usize>,
}

#[derive(Debug)]
pub struct Arena {
    nodes: Vec<NtfsNode>
}

impl Arena {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
        }
    }

    fn add_node(&mut self, name: &str, parent: Option<usize>) -> usize {
        let node_id = self.nodes.len();
        self.nodes.push(NtfsNode {
            name: name.to_string(),
            parent,
            children: Vec::new(),
        });
        if let Some(parent_id) = parent {
            self.nodes[parent_id].children.push(node_id);
        }
        node_id
    }

    fn get(&self, node_id: usize) -> &NtfsNode {
        &self.nodes[node_id]
    }
}

#[derive(Debug)]
pub struct TreeNavigator {
    arena: Arena,
    current: usize,
    history: Vec<usize>,
}

impl TreeNavigator {
    pub fn new(start: usize) -> Self {
        Self {
            arena: Arena::new(),
            current: start,
            history: Vec::new(),
        }
    }

    fn current(&self) -> &NtfsNode {
        &self.arena.nodes[self.current]
    }

    fn get(&self, node_id: usize) -> &NtfsNode {
        &self.arena.get(node_id)
    }

    pub fn create_root(&mut self, name: &str) {
        let node_id=self.arena.add_node(name, None);
        self.enter(node_id);
    }

    fn enter(&mut self, node_id: usize) {
        self.history.push(self.current);
        self.current = node_id;
    }

    pub fn add(&mut self, name: &str) {
        self.arena.add_node(name, Some(self.current));
    }

    fn add_and_enter(&mut self, name: &str) {
        let node_id = self.arena.add_node(name, Some(self.current));
        self.enter(node_id);
    }

    fn pop(&mut self) {
        if let Some(previous) = self.history.pop() {
            self.current = previous
        }
    }
}

#[test]
fn tree_test() {
    let mut nav = TreeNavigator::new(0);
    nav.create_root("Root");
    nav.add_and_enter("Folder 1");
    nav.add("File 1.txt");
    nav.pop();
    nav.add("Folder 2");
    nav.pop();

    println!("nav: {:#?}", nav);
    println!("nav: {:#?}", nav.arena.nodes);
}