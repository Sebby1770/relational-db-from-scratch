use std::ops::RangeInclusive;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BPlusTree<K, V>
where
    K: Ord + Clone,
    V: Clone,
{
    order: usize,
    root: Node<K, V>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Node<K, V>
where
    K: Ord + Clone,
    V: Clone,
{
    Leaf {
        entries: Vec<(K, V)>,
    },
    Internal {
        keys: Vec<K>,
        children: Vec<Node<K, V>>,
    },
}

impl<K, V> BPlusTree<K, V>
where
    K: Ord + Clone,
    V: Clone,
{
    pub fn new(order: usize) -> Self {
        assert!(order >= 3, "B+ tree order must be at least 3");
        Self {
            order,
            root: Node::Leaf {
                entries: Vec::new(),
            },
        }
    }

    pub fn insert(&mut self, key: K, value: V) {
        if let Some((separator, right)) = self.root.insert(key, value, self.order) {
            let old_root = std::mem::replace(
                &mut self.root,
                Node::Leaf {
                    entries: Vec::new(),
                },
            );
            self.root = Node::Internal {
                keys: vec![separator],
                children: vec![old_root, right],
            };
        }
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.root.get(key)
    }

    pub fn range(&self, range: RangeInclusive<K>) -> Vec<(K, V)> {
        let mut output = Vec::new();
        self.root.range(&range, &mut output);
        output
    }

    pub fn height(&self) -> usize {
        self.root.height()
    }
}

impl<K, V> Node<K, V>
where
    K: Ord + Clone,
    V: Clone,
{
    fn insert(&mut self, key: K, value: V, order: usize) -> Option<(K, Node<K, V>)> {
        match self {
            Node::Leaf { entries } => {
                match entries.binary_search_by(|(existing, _)| existing.cmp(&key)) {
                    Ok(index) => entries[index] = (key, value),
                    Err(index) => entries.insert(index, (key, value)),
                }

                if entries.len() <= order {
                    return None;
                }

                let right_entries = entries.split_off(entries.len() / 2);
                let separator = right_entries
                    .first()
                    .expect("split leaf has right entries")
                    .0
                    .clone();
                Some((
                    separator,
                    Node::Leaf {
                        entries: right_entries,
                    },
                ))
            }
            Node::Internal { keys, children } => {
                let child_index = keys.partition_point(|separator| &key >= separator);
                if let Some((separator, right_child)) =
                    children[child_index].insert(key, value, order)
                {
                    keys.insert(child_index, separator);
                    children.insert(child_index + 1, right_child);
                }

                if keys.len() <= order {
                    return None;
                }

                let mid = keys.len() / 2;
                let separator = keys[mid].clone();
                let right_keys = keys.split_off(mid + 1);
                keys.truncate(mid);
                let right_children = children.split_off(mid + 1);

                Some((
                    separator,
                    Node::Internal {
                        keys: right_keys,
                        children: right_children,
                    },
                ))
            }
        }
    }

    fn get(&self, key: &K) -> Option<&V> {
        match self {
            Node::Leaf { entries } => entries
                .binary_search_by(|(existing, _)| existing.cmp(key))
                .ok()
                .map(|index| &entries[index].1),
            Node::Internal { keys, children } => {
                let child_index = keys.partition_point(|separator| key >= separator);
                children[child_index].get(key)
            }
        }
    }

    fn range(&self, range: &RangeInclusive<K>, output: &mut Vec<(K, V)>) {
        match self {
            Node::Leaf { entries } => {
                for (key, value) in entries {
                    if range.contains(key) {
                        output.push((key.clone(), value.clone()));
                    }
                }
            }
            Node::Internal { children, .. } => {
                for child in children {
                    child.range(range, output);
                }
            }
        }
    }

    fn height(&self) -> usize {
        match self {
            Node::Leaf { .. } => 1,
            Node::Internal { children, .. } => 1 + children[0].height(),
        }
    }
}
