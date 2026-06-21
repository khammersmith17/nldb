use crate::disk::DiskRecord;
use crate::error::MemtableError;
use crate::sstable;
use crate::wal::Wal;
use crate::wal::WalIterator;
use std::cmp::Ordering;
use std::fs::File;
use std::path::Path;

/*
  1. Every node is either red or black
  2. The root is black
  3. Every NIL leaf is black
  4. If a node is red, both its children are black
  5. All paths from any node to its descendant NIL leaves contain the same number of black nodes (uniform black-height)

  Plus the BST property: left subtree keys < node key < right subtree keys.

  Rotations (the core structural operation):
  - Left rotate on N: N's right child takes N's place, N becomes its left child
  - Right rotate on N: N's left child takes N's place, N becomes its right child
  - Rotations preserve BST order

  Insert fixup (after inserting a red node):
  - Case 1: Uncle is red → recolor parent + uncle black, grandparent red, move up
  - Case 2: Uncle is black, node is inner child → rotate parent, becomes Case 3
  - Case 3: Uncle is black, node is outer child → rotate grandparent, swap colors of parent/grandparent

  Delete fixup (after removing a black node, propagate "double black"):
  - Case 1: Sibling is red → rotate parent, reduces to Cases 2–4
  - Case 2: Sibling is black, both sibling's children black → recolor sibling red, move double-black up
  - Case 3: Sibling is black, near child red, far child black → rotate sibling, reduces to Case 4
  - Case 4: Sibling is black, far child red → rotate parent, done
* */

// Determine inner vs outer child.
#[inline]
fn child_type(
    grandparent_left: Option<usize>,
    parent: usize,
    parent_left: Option<usize>,
    node: usize,
) -> ChildType {
    let is_parent_left = grandparent_left == Some(parent);
    let is_node_left = parent_left == Some(node);

    if is_parent_left == is_node_left {
        ChildType::Outer
    } else {
        ChildType::Inner
    }
}

fn derive_inner_rotation(grandparent_left: Option<usize>, parent: usize) -> RotationDirection {
    // Left right zig zag left rotate parent.
    // Otherwise right rotate parent.
    if grandparent_left == Some(parent) {
        // Left-Right zig zag.
        RotationDirection::Left
    } else {
        RotationDirection::Right
    }
}

fn derive_outer_rotation(grandparent_left: Option<usize>, parent: usize) -> RotationDirection {
    // Left - Left right rotate.
    if grandparent_left == Some(parent) {
        RotationDirection::Right
    } else {
        RotationDirection::Left
    }
}

// Determine inner vs outer child and the rotation direction.
fn derive_rotation_direction(
    grandparent_left: Option<usize>,
    parent: usize,
    parent_left: Option<usize>,
    node: usize,
) -> (ChildType, RotationDirection) {
    match child_type(grandparent_left, parent, parent_left, node) {
        ChildType::Outer => (
            ChildType::Outer,
            derive_outer_rotation(grandparent_left, parent),
        ),
        ChildType::Inner => (
            ChildType::Inner,
            derive_inner_rotation(grandparent_left, parent),
        ),
    }
}

enum ChildType {
    Inner,
    Outer,
}

enum RotationDirection {
    Left,
    Right,
}

enum Child {
    Left,
    Right,
}

enum InsertionPosition {
    NewNode(usize, Child), // Parent index.
    ExistingNode(usize),   // Index of node to mutate.
}

#[derive(Debug, PartialEq, Eq)]
enum Color {
    Red,
    Black,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum NodeData {
    Data(Vec<u8>),
    Tombstone,
}

impl NodeData {
    fn len(&self) -> usize {
        if let NodeData::Data(data) = self {
            data.len()
        } else {
            0_usize
        }
    }
}

#[derive(Debug, Eq)]
pub struct MemtableNode {
    pub key: String,
    pub data: NodeData,
    pub left: Option<usize>,
    pub right: Option<usize>,
    parent: Option<usize>,
    color: Color,
}

impl PartialEq for MemtableNode {
    fn eq(&self, other: &MemtableNode) -> bool {
        self.key == other.key
    }
}

impl PartialOrd for MemtableNode {
    fn partial_cmp(&self, other: &MemtableNode) -> Option<Ordering> {
        self.key.partial_cmp(&other.key)
    }
}

impl Ord for MemtableNode {
    fn cmp(&self, other: &MemtableNode) -> Ordering {
        self.key.cmp(&other.key)
    }
}

impl MemtableNode {
    // Returns the index of a nodes uncle, ie the other Node that descends from grandparent in the
    // tree.
    fn uncle(&self, arena: &[MemtableNode]) -> Option<usize> {
        let parent = &arena[self.parent?];
        let grandparent = &arena[parent.parent?];

        // If parent index is grandparents left, uncle is right, otherwise uncle is left.
        if grandparent.left == self.parent {
            grandparent.right
        } else {
            grandparent.left
        }
    }

    fn update_data_from_node(&mut self, node: MemtableNode) {
        self.data = node.data;
    }

    fn new(key: String, data: NodeData) -> MemtableNode {
        MemtableNode {
            key,
            data,
            left: None,
            right: None,
            parent: None,
            color: Color::Red,
        }
    }
}

pub struct MemtableInner {
    pub arena: Vec<MemtableNode>,
    max_size: usize,
    pub root_node: Option<usize>,
    pub current_size: usize,
    wal: Wal,
}

impl MemtableInner {
    pub fn new(max_size: usize, max_nodes: usize) -> Result<MemtableInner, std::io::Error> {
        let arena = Vec::with_capacity(max_nodes);
        let wal = Wal::new()?;

        Ok(MemtableInner {
            arena,
            max_size,
            root_node: None,
            current_size: 0_usize,
            wal,
        })
    }

    pub fn from_wal(
        wal_filepath: &Path,
        max_size: usize,
        max_nodes: usize,
    ) -> std::io::Result<MemtableInner> {
        let wal_iter = WalIterator::new(wal_filepath)?;
        let mut table = Self::new(max_size, max_nodes)?;

        for record in wal_iter {
            let DiskRecord { key, data } = record;
            // SAFETY: Recovering from WAL should not exceed max memtable size.
            let _ = table.insert(key, data);
        }

        std::fs::remove_file(wal_filepath)?;
        Ok(table)
    }

    fn full(&self) -> bool {
        self.arena.len() == self.arena.capacity() || self.current_size >= self.max_size
    }

    pub fn flush_to_disk(self, fd: &mut File) -> std::io::Result<Self> {
        sstable::encode::write_sstable(&self, fd)?;
        let max_size = self.max_size;
        let max_nodes = self.arena.len();
        Self::new(max_size, max_nodes)
    }

    // If full -> flush.
    pub fn insert(&mut self, key: String, value: NodeData) -> Result<(), MemtableError> {
        if self.full() {
            return Err(MemtableError::TableFull);
        }

        let node = MemtableNode::new(key, value);
        self.wal.write_log(&node);

        // Insert node into tree.
        self.insert_node(node);
        Ok(())
    }

    fn insert_node(&mut self, mut node: MemtableNode) {
        /*
         * Insertion:
         *   1. Check root existance
         *       If not root, set and return
         *   2. Find insert position in the tree
         *       If a node with the same value exists in the tree, update that node
         *       and return
         *   3. With the parent and left/right child
         *       give the new node the parent pointer
         *       update the parent with the new nodes pointer
         *    4. Push the node into the buffer
         *    5. Resolve Red black tree structure
         * */
        // The nodes destination index in the buffer.
        let node_idx = self.arena.len();

        // Tree is empty, insert at root and change color.
        if self.root_node.is_none() {
            self.set_root(node, node_idx);
            return;
        }

        let (parent_ptr, child) = match self.find_insert_position(&node) {
            InsertionPosition::ExistingNode(curr_idx) => {
                self.arena[curr_idx].update_data_from_node(node);
                return;
            }
            InsertionPosition::NewNode(parent, child) => (parent, child),
        };

        let parent_node = &mut self.arena[parent_ptr];
        match child {
            Child::Left => parent_node.left = Some(node_idx),
            Child::Right => parent_node.right = Some(node_idx),
        }

        node.parent = Some(parent_ptr);
        self.current_size += node.data.len();
        self.arena.push(node);
        self.insert_fix_tree(node_idx);
    }

    fn set_root(&mut self, mut node: MemtableNode, node_idx: usize) {
        self.root_node = Some(node_idx); // Should be 0.
        node.color = Color::Black;
        self.arena.push(node);
    }

    fn find_insert_position(&self, key: &MemtableNode) -> InsertionPosition {
        let mut curr = self.root_node.unwrap();

        #[allow(unused_assignments)]
        let mut parent = curr;

        loop {
            parent = curr;
            let node = &self.arena[curr];
            match key.cmp(&node) {
                Ordering::Equal => {
                    return InsertionPosition::ExistingNode(curr);
                }
                Ordering::Less => {
                    if let Some(child_idx) = node.left {
                        curr = child_idx;
                    } else {
                        return InsertionPosition::NewNode(parent, Child::Left);
                    }
                }
                Ordering::Greater => {
                    if let Some(child_idx) = node.right {
                        curr = child_idx;
                    } else {
                        return InsertionPosition::NewNode(parent, Child::Right);
                    }
                }
            }
        }
    }

    fn insert_fix_tree(&mut self, mut node_idx: usize) {
        loop {
            // Case 1
            let Some(parent_idx) = self.arena[node_idx].parent else {
                break;
            };

            if self.arena[parent_idx].color == Color::Black {
                break;
            }

            // SAFETY: Grandparent exists as parent is Red, thus not root.
            let grandparent_idx = self.arena[parent_idx].parent.unwrap();
            match (&self.arena[node_idx]).uncle(&self.arena) {
                Some(u) if self.arena[u].color == Color::Red => {
                    self.arena[u].color = Color::Black;
                    self.arena[parent_idx].color = Color::Black;
                    self.arena[grandparent_idx].color = Color::Red;
                    node_idx = grandparent_idx;
                }
                _ => {
                    // Uncle is black.
                    match derive_rotation_direction(
                        self.arena[grandparent_idx].left,
                        parent_idx,
                        self.arena[parent_idx].left,
                        node_idx,
                    ) {
                        // Case 2: Rotate parent.
                        (ChildType::Inner, RotationDirection::Right) => {
                            self.right_rotate(parent_idx);
                            node_idx = parent_idx;
                        }
                        (ChildType::Inner, RotationDirection::Left) => {
                            self.left_rotate(parent_idx);
                            node_idx = parent_idx;
                        }
                        // Case 3: Rotate grandparent.
                        (ChildType::Outer, RotationDirection::Right) => {
                            self.insert_recolor(parent_idx, grandparent_idx);
                            self.right_rotate(grandparent_idx);
                            break;
                        }
                        (ChildType::Outer, RotationDirection::Left) => {
                            self.insert_recolor(parent_idx, grandparent_idx);
                            self.left_rotate(grandparent_idx);
                            break;
                        }
                    }
                }
            }
        }

        // Rule 2: root is always black.
        self.arena[self.root_node.unwrap()].color = Color::Black;
    }

    fn insert_recolor(&mut self, parent: usize, grandparent: usize) {
        self.arena[parent].color = Color::Black;
        self.arena[grandparent].color = Color::Red;
    }

    fn left_rotate(&mut self, n: usize) {
        let r = self.arena[n]
            .right
            .expect("Right child needs to exists for left rotation");

        // R's left child becomes N's right child.
        self.arena[n].right = self.arena[r].left;
        if let Some(r_left) = self.arena[r].left {
            self.arena[r_left].parent = Some(n)
        };

        // Swap R and N.
        self.arena[r].parent = self.arena[n].parent;
        match self.arena[n].parent {
            Some(p) => {
                if self.arena[p].left == Some(n) {
                    self.arena[p].left = Some(r)
                } else {
                    self.arena[p].right = Some(r)
                }
            }
            None => self.root_node = Some(r),
        }

        self.arena[r].left = Some(n);
        self.arena[n].parent = Some(r);
    }

    fn right_rotate(&mut self, n: usize) {
        let l = self.arena[n]
            .left
            .expect("Left child needs to exists for right rotations");

        // L's right child becomes N's left child.
        self.arena[n].left = self.arena[l].right;
        if let Some(l_right) = self.arena[l].right {
            self.arena[l_right].parent = Some(n)
        }

        // Swap L and N.
        self.arena[l].parent = self.arena[n].parent;
        match self.arena[n].parent {
            Some(p) => {
                if self.arena[p].left == Some(n) {
                    self.arena[p].left = Some(l)
                } else {
                    self.arena[p].right = Some(l)
                }
            }
            None => self.root_node = Some(l),
        }
        self.arena[l].right = Some(n);
        self.arena[n].parent = Some(l);
    }

    // Always returned owned copy of the data segment.
    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        let node_idx = self.get_search(key)?;
        let node_data = &self.arena[node_idx].data;
        match node_data {
            NodeData::Data(data) => Some(data.clone()),
            NodeData::Tombstone => None,
        }
    }

    fn get_search(&self, key: &str) -> Option<usize> {
        let mut curr = self.root_node?;

        loop {
            let curr_node = &self.arena[curr];
            match key.cmp(&curr_node.key) {
                Ordering::Equal => return Some(curr),
                Ordering::Less => {
                    if let Some(left) = curr_node.left {
                        curr = left;
                    } else {
                        return None;
                    }
                }
                Ordering::Greater => {
                    if let Some(right) = curr_node.right {
                        curr = right
                    } else {
                        return None;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_memtable() -> MemtableInner {
        let wal = Wal::new().unwrap();
        MemtableInner {
            arena: Vec::with_capacity(64),
            max_size: usize::MAX,
            root_node: None,
            current_size: 0,
            wal,
        }
    }

    // Recursively checks RB invariants, returns the black-height of the subtree.
    fn check_subtree(arena: &[MemtableNode], node: Option<usize>) -> usize {
        let Some(idx) = node else {
            return 1; // NIL counts as black
        };
        let n = &arena[idx];
        if n.color == Color::Red {
            assert!(
                n.left.map_or(true, |l| arena[l].color == Color::Black),
                "Red node {idx} has red left child"
            );
            assert!(
                n.right.map_or(true, |r| arena[r].color == Color::Black),
                "Red node {idx} has red right child"
            );
        }
        let lbh = check_subtree(arena, n.left);
        let rbh = check_subtree(arena, n.right);
        assert_eq!(lbh, rbh, "Black height mismatch at node {idx}");
        if n.color == Color::Black {
            lbh + 1
        } else {
            lbh
        }
    }

    fn check_invariants(t: &MemtableInner) {
        let Some(root) = t.root_node else { return };
        assert_eq!(t.arena[root].color, Color::Black, "Root must be black");
        check_subtree(&t.arena, Some(root));
    }

    #[test]
    fn test_root_is_black() {
        let mut t = make_memtable();
        t.insert("m".to_string(), NodeData::Data(b"v".to_vec()))
            .unwrap();
        assert_eq!(t.arena[t.root_node.unwrap()].color, Color::Black);
    }

    #[test]
    fn test_get_after_insert() {
        let mut t = make_memtable();
        t.insert("hello".to_string(), NodeData::Data(b"world".to_vec()))
            .unwrap();
        assert_eq!(t.get("hello"), Some(b"world".to_vec()));
        assert_eq!(t.get("missing"), None);
    }

    #[test]
    fn test_duplicate_key_updates_value() {
        let mut t = make_memtable();
        t.insert("key".to_string(), NodeData::Data(b"v1".to_vec()))
            .unwrap();
        t.insert("key".to_string(), NodeData::Data(b"v2".to_vec()))
            .unwrap();
        assert_eq!(t.get("key"), Some(b"v2".to_vec()));
    }

    #[test]
    fn test_case1_uncle_red() {
        // m(B) → e(R) left, t(R) right, then a(R) left of e
        // parent e is red, uncle t is red → Case 1: recolor e,t black, m red → m forced black
        let mut t = make_memtable();
        for k in ["m", "e", "t", "a"] {
            t.insert(k.to_string(), NodeData::Data(b"".to_vec()))
                .unwrap();
        }
        check_invariants(&t);
        let root = t.root_node.unwrap();
        let e_idx = t.arena[root].left.unwrap();
        let t_idx = t.arena[root].right.unwrap();
        assert_eq!(t.arena[e_idx].color, Color::Black);
        assert_eq!(t.arena[t_idx].color, Color::Black);
    }

    #[test]
    fn test_case3_right_right() {
        // a → b → c: right-right straight line → left rotate a, b becomes root
        let mut t = make_memtable();
        for k in ["a", "b", "c"] {
            t.insert(k.to_string(), NodeData::Data(b"".to_vec()))
                .unwrap();
        }
        check_invariants(&t);
        let root = t.root_node.unwrap();
        assert_eq!(t.arena[root].key, "b");
        assert_eq!(t.arena[root].color, Color::Black);
    }

    #[test]
    fn test_case3_left_left() {
        // c → b → a: left-left straight line → right rotate c, b becomes root
        let mut t = make_memtable();
        for k in ["c", "b", "a"] {
            t.insert(k.to_string(), NodeData::Data(b"".to_vec()))
                .unwrap();
        }
        check_invariants(&t);
        let root = t.root_node.unwrap();
        assert_eq!(t.arena[root].key, "b");
        assert_eq!(t.arena[root].color, Color::Black);
    }

    #[test]
    fn test_case2_case3_right_left() {
        // a → c → b: right-left zigzag → Case 2 (right rotate c) then Case 3, b becomes root
        let mut t = make_memtable();
        for k in ["a", "c", "b"] {
            t.insert(k.to_string(), NodeData::Data(b"".to_vec()))
                .unwrap();
        }
        check_invariants(&t);
        let root = t.root_node.unwrap();
        assert_eq!(t.arena[root].key, "b");
        assert_eq!(t.arena[root].color, Color::Black);
    }

    #[test]
    fn test_case2_case3_left_right() {
        // c → a → b: left-right zigzag → Case 2 (left rotate a) then Case 3, b becomes root
        let mut t = make_memtable();
        for k in ["c", "a", "b"] {
            t.insert(k.to_string(), NodeData::Data(b"".to_vec()))
                .unwrap();
        }
        check_invariants(&t);
        let root = t.root_node.unwrap();
        assert_eq!(t.arena[root].key, "b");
        assert_eq!(t.arena[root].color, Color::Black);
    }

    #[test]
    fn test_invariants_many_inserts() {
        let mut t = make_memtable();
        for k in ["f", "b", "g", "a", "d", "i", "c", "e", "h"] {
            t.insert(k.to_string(), NodeData::Data(b"".to_vec()))
                .unwrap();
            check_invariants(&t);
        }
    }

    #[test]
    fn test_all_keys_retrievable() {
        let mut t = make_memtable();
        let keys = ["f", "b", "g", "a", "d", "i", "c", "e", "h"];
        for k in keys {
            t.insert(
                k.to_string(),
                NodeData::Data(vec![*k.as_bytes().first().unwrap()]),
            )
            .unwrap();
        }
        for k in keys {
            assert_eq!(
                t.get(k),
                Some(vec![*k.as_bytes().first().unwrap()]),
                "Missing key {k}"
            );
        }
    }
}
