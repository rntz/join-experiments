// (Mostly Claude-generated with minimal review.)
//
// A trivial in-memory Database backed by Vecs. Shared by the benchmark example and the
// integration tests. Note this uses std's SipHash-backed HashMap for the name -> relation
// lookup; the fast FxHash only matters inside the trie indexes.
use std::collections::HashMap;

use crate::{Value, Database};

// A relation's rows, flattened: row k is data[k*arity .. (k+1)*arity]. We keep `count`
// explicitly because a zero-arity relation's row count can't be recovered from `data`.
struct Rel {
    arity: usize,
    count: usize,
    data: Vec<Value>,
}

pub struct VecDb {
    rels: HashMap<&'static str, Rel>,
}

impl VecDb {
    pub fn new() -> Self { VecDb { rels: HashMap::new() } }

    // Builder-style: add a relation. Panics if a row's width != arity.
    pub fn rel(mut self, name: &'static str, arity: usize, rows: Vec<Vec<Value>>) -> Self {
        for row in &rows { assert_eq!(row.len(), arity, "bad row width in {name}"); }
        let count = rows.len();
        let data: Vec<Value> = rows.into_iter().flatten().collect();
        self.rels.insert(name, Rel { arity, count, data });
        self
    }
}

impl Default for VecDb {
    fn default() -> Self { Self::new() }
}

impl Database for VecDb {
    type Rel = &'static str;
    fn arity(&self, r: &'static str) -> usize { self.rels[r].arity }
    fn count(&self, r: &'static str) -> usize { self.rels[r].count }
    fn scan<F: FnMut(&[Value])>(&self, r: &'static str, mut process_row: F) {
        let rel = &self.rels[r];
        for k in 0..rel.count { process_row(&rel.data[k * rel.arity..(k + 1) * rel.arity]) }
    }
}
