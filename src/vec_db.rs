// (Mostly Claude-generated with minimal review.)
//
// A trivial in-memory Database backed by Vecs. Shared by the benchmark example and the
// integration tests. Note this uses std's SipHash-backed HashMap for the name -> relation
// lookup; the fast FxHash only matters inside the trie indexes.
use std::collections::HashMap;

use crate::join::{Database, Value};

pub struct VecDb {
    // name -> (arity, rows)
    rels: HashMap<&'static str, (usize, Vec<Vec<Value>>)>,
}

impl VecDb {
    pub fn new() -> Self { VecDb { rels: HashMap::new() } }

    // Builder-style: add a relation. Panics if a row's width != arity.
    pub fn rel(mut self, name: &'static str, arity: usize, rows: Vec<Vec<Value>>) -> Self {
        for row in &rows { assert_eq!(row.len(), arity, "bad row width in {name}"); }
        self.rels.insert(name, (arity, rows));
        self
    }
}

impl Default for VecDb {
    fn default() -> Self { Self::new() }
}

impl Database for VecDb {
    type RelId = &'static str;
    fn arity(&self, r: &'static str) -> usize { self.rels[r].0 }
    fn count(&self, r: &'static str) -> usize { self.rels[r].1.len() }
    fn rows(&self, r: &'static str) -> impl Iterator<Item = &[Value]> {
        self.rels[r].1.iter().map(|row| row.as_slice())
    }
}
