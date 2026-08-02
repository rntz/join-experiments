#![allow(missing_docs, dead_code)]

use std::io::prelude::*;
use std::time::Instant;
use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::Hash;

macro_rules! print_flush {
    ($($e:tt)*) => { { print!($($e)*); std::io::stdout().flush().unwrap() } }
}


// ---------- STEPS FOR EXECUTING A QUERY ----------
//
// 0. Intern all values so everything is usize and equality is equality. This avoids
//    needing to put tags on things.
//
//    Use one hashtable & counter for each entity & attribute type. This can be
//    incrementally maintained.
//
//    Alternatives: tag every single value and dispatch every single time (slow but maybe
//    not bad enough to matter); or try to be cleverer about tag placement, eg tag every
//    column and dispatch once per for-loop (probs ok perf but a pain in the ass to do).
//
// 1. CHASING FDS GOES HERE?
//    I think chasing FDs may be more important than semijoin reduction if I
//    only have time to do one.
//
// 2. SEMIJOIN REDUCTION GOES HERE?
//
// 3. Get statistics on it, eg. for each variable, the min across all relations
//    of the # of values it could have. We can approximate that using the size
//    of the relation, but can do even better by actually counting distinct
//    values.
//
// 4. Use these stats (& FDs once we have them) to pick a variable order.
//
// 5. Build trie indexes on each relation based on the query & var order.
//    DONE: have Trie::build to build a single index.
//    TODO: derive which indexes to build based on a query & variable order,
//    shove them in a struct.
//
// 6. Execute query using the indexes.
//    DONE: see QueryPlan::execute_dfs
//    TODO: a breadth-first version?


// ---------- HASHER SELECTION ----------
//
// Hash algorithm impacts join performance heavily (factor of ~3x). Rust's default is slow
// in exchange for extra security against adversarial attacks. We probably don't need
// this? So we use a much simpler, faster hash, FxHash. Would be fine to replace this with
// a library, presumably there's some crate in the Rust ecosystem for this.
//
// Pick your hash algorithm by changing "type HashBuilder":
type HashBuilder = FxBuildHasher; // fast, non-cryptographic hash
// type HashBuilder = std::collections::hash_map::RandomState; // stdlib SipHash

type Map<K, V> = HashMap<K, V, HashBuilder>;
type Set<K> = HashSet<K, HashBuilder>;

// An implementation of FxHash (Claude-generated).
const FX_SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;
const FX_ROTATE: u32 = 5;

#[derive(Default)]
struct FxHasher { hash: u64 }

impl FxHasher {
    #[inline]
    fn add(&mut self, i: u64) {
        self.hash = (self.hash.rotate_left(FX_ROTATE) ^ i).wrapping_mul(FX_SEED);
    }
}

impl std::hash::Hasher for FxHasher {
    #[inline]
    fn finish(&self) -> u64 { self.hash }
    #[inline]
    fn write_usize(&mut self, i: usize) { self.add(i as u64); }
    #[inline]
    fn write_u64(&mut self, i: u64) { self.add(i); }
    #[inline]
    fn write_u32(&mut self, i: u32) { self.add(i as u64); }
    #[inline]
    fn write_u8(&mut self, i: u8) { self.add(i as u64); }
    #[inline]
    fn write(&mut self, mut bytes: &[u8]) {
        while bytes.len() >= 8 {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(&bytes[..8]);
            self.add(u64::from_le_bytes(buf));
            bytes = &bytes[8..];
        }
        if !bytes.is_empty() {
            let mut buf = [0u8; 8];
            buf[..bytes.len()].copy_from_slice(bytes);
            self.add(u64::from_le_bytes(buf));
        }
    }
}

#[derive(Default, Clone)]
struct FxBuildHasher;
impl std::hash::BuildHasher for FxBuildHasher {
    type Hasher = FxHasher;
    #[inline]
    fn build_hasher(&self) -> FxHasher { FxHasher::default() }
}


// ---------- DATABASES AND QUERIES ----------
//
// I'm assuming we intern everything up front. This makes things simpler than figuring out
// where to put tags to minimize tag-checking overhead.
type Value = usize;

// A database is something which has relations and can tabulate them.
//
// TODO: how do I incorporate functional dependency information here?
//
// ANSWER: simplest way is to let each relation declare a primary key, which all other
// keys are determined by. This is less general than full FDs but easier to represent and
// plan around and handles ACSet-type schemas.
trait Database {
    type RelId: Eq + Hash + Clone;
    fn arity(&self, r: Self::RelId) -> usize;
    fn count(&self, r: Self::RelId) -> usize;
    fn rows(&self, r: Self::RelId) -> impl Iterator<Item = &[Value]>;
}

// impl<Db: Database> Database for &Db {
//     type RelId = Db::RelId;
//     fn arity(&self, r: Db::RelId) -> usize { (*self).arity(r) }
//     fn count(&self, r: Db::RelId) -> usize { (*self).count(r) }
//     fn rows(&self, r: Db::RelId) -> impl Iterator<Item = &[Value]> { (*self).rows(r) }
// }

struct Query<Db: Database, Var: Eq + Hash + Copy> {
    vars: Vec<Var>,
    atoms: Vec<Atom<Db::RelId, Var>>,
}

// TODO: how do we represent constants in atoms?
struct Atom<RelId, Var> {
    relation: RelId,
    vars: Vec<Var>,
}

// ==== ON IMPLEMENTING COMPUTATIONAL ATOMS ====
//
// Frank McSherry's DataToad project takes a "breadth-first" approach to solving WCOJs,
// maintaining a vector of partial solutions for the first N variables, then extending to
// partial solutions for N+1, etc. LFTJ takes a "depth first" or backtracking approach
// instead. Both of these can incorporate computational atoms:
//
// - Frank McSherry has a blog post about how to plan & execute computational atoms:
//
//   https://github.com/frankmcsherry/blog/blob/master/posts/2025-12-23.md
//
//   Email me (Michael Arntzenius, daekharel@gmail.com) if you're having trouble
//   understanding it or how it relates to this implementation; or email Frank and cc me,
//   he's quite friendly (but won't know anything about this implementation).
//
// - The Leapfrog Triejoin paper discusses some kinds of computational atoms:
//
//   https://arxiv.org/abs/1210.0481
//
//   LFTJ uses a "trie iterator" interface. If you line things up right it's possible for
//   many computational atoms to satisfy this interface. You can think of this as
//   materializing the trie lazily/on-demand. Of course, computational atoms can't
//   materialize levels of the trie that correspond to their *input* variables, but they
//   can be told to "seek to position x" (this assigns that input variable to x). As long
//   as *some* atom/trie iterator can materialize a list of candidates for this variable,
//   things work out eventually.
//
//   See section 3.4, p6, list item 1, which discusses equality atoms, and section 6.2,
//   numbered list, elements 2-3 ("Functions", "Primitives") and 6 ("Ranges"). (Note that
//   "Function" does NOT mean computational function here: it means functionaln
//   dependency.)


// ---------- TRIE INDEXES ----------
enum Trie {
    Leaf,
    Node(TrieMap),
}
type TrieMap = Map<Value, Trie>;

// ==== LONG ASIDE ABOUT LEAPFROG TRIEJOIN AND SORTING-BASED APPROACHES TO WCOJS ====
//
// This is the hash-based or nested approach to trie indexes. (Of course, we could use a
// BTreeMap instead for each trie level but that is essentially the same approach.)
//
// An alternative approach is to use a sorted index for the entire "trie". For instance, a
// vector sorted in lexical order can be treated as a "trie" implicitly. This results in a
// join that looks more like Leapfrog Triejoin (LFTJ), where we intersect iterators at a
// given trie level by round-robinning between them, advancing the most lagging iterators
// toward the most advanced one until they all agree. This is how dijkstralog works, for
// instance (https://github.com/rntz/dijkstralog).
//
// Unfortunately, while sorted vectors are simple and efficient, they can't be updated
// in-place efficiently. So you'll need a B-tree, LSM-tree, or similar. These are quite a
// bit more complicated to implement and work with than a hash-based Trie. (My vague
// feeling, not substantiated by any actual benchmarking, is that B-trees are better for
// reads and worse for large writes than LSM-trees. Dijkstralog contains an LSM
// implementation.)
//
// You might think we could reuse the Rust stdlib's BTreeMap. Unfortunately, it doesn't
// support the primitives needed for LFTJ: we need to be able to keep an internal iterator
// into the BTree that can efficiently "seek" forward toward an upper bound. We can't do
// this with standard iterators. There's an experimental "Cursor" interface on BTrees
// available on Rust nightly (as of 2026-07-31) that gets partway there:
//
// https://github.com/rust-lang/rust/issues/107540
// https://doc.rust-lang.org/std/collections/btree_map/struct.Cursor.html
// https://doc.rust-lang.org/std/collections/struct.BTreeMap.html#method.lower_bound
//
// But I think this interface isn't rich enough to handle LFTJ: it doesn't support seeking
// an existing Cursor forward toward a bound. (Also, for proper asymptotics on your join
// you want the search implementation to use galloping/exponential search, not binary
// search; I'm not sure which they're using. Using binary search can make dense joins,
// where many values match, quite inefficient.)


// ---------- ON TRIE INDEXING FOR WCOJs ----------
//
// Each relation may need multiple trie indexes, because with a single variable order
// different atoms may traverse it differently, e.g. S(x,y) S(y,x). These indexes will in
// general not be simply permutations of the variable order, for two reasons:
//
//     1. Constants, eg: S(x,2).
//     2. Variable re-occurrences, eg: S(x,x).
//
// A trie index for an atom R(xs...) will have N levels, where N is the # of distinct
// variables in xs. An index can be specified by indicating what to do for each column of
// the relation.

type IndexShape = Vec<IndexColumnShape>; // length = arity of relation
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum IndexColumnShape {   // what to do with column i.
    TrieLevel(usize), // TrieLevel(k) => becomes trie level k.
    EqConst(Value),   // EqConst(v)   => filter: equal to v, otherwise discard.
    EqColumn(usize),  // EqColumn(j)  => filter: equal to column j, otherwise discard.
}

// INVARIANT: If shape[i] = EqColumn(j), we should have j < i and shape[j] = TrieLevel(_).
// This ensures that shapes which denote equivalent indexes are equal on the nose.

impl Trie {
    // Trie::build() returns None if the trie is empty. This is necessary to distinguish
    // between Some(Trie::Leaf()), a trie containing an single empty tuple, and None, an
    // empty trie.
    fn build<Db: Database>(db: &Db, rel: Db::RelId, shape: &IndexShape) -> Option<Trie> {
        // Preprocess `shape` into:
        //  - `level_to_col[k]` is the column that becomes trie level k.
        //  - `filters` are the columns carrying EqConst/EqColumn checks.
        let n_levels = shape.iter().filter(|c| matches!(c, IndexColumnShape::TrieLevel(_))).count();
        let mut level_to_col: Vec<Option<usize>> = vec![None; n_levels];
        let mut filters: Vec<(usize, &IndexColumnShape)> = Vec::new();
        for (col, colshape) in shape.iter().enumerate() {
            match colshape {
                IndexColumnShape::TrieLevel(k) => {
                    // The TrieLevels must form a permutation of 0..n_levels: each level in
                    // range and assigned exactly once.
                    assert!(*k < n_levels, "TrieLevel({k}) out of range for {n_levels} levels");
                    assert!(level_to_col[*k].is_none(), "TrieLevel({k}) assigned twice");
                    level_to_col[*k] = Some(col);
                }
                IndexColumnShape::EqConst(_) => filters.push((col, colshape)),
                IndexColumnShape::EqColumn(j) => {
                    // Enforce the invariant on Shape.
                    assert!(*j < col, "EqColumn({j}) at column {col} is not a backreference");
                    assert!(matches!(shape[*j], IndexColumnShape::TrieLevel(_)),
                            "EqColumn({j}) at column {col} must point to a TrieLevel");
                    filters.push((col, colshape));
                }
            }
        }
        // Every level got a column (follows from the count + range + no-dup asserts, but
        // make it explicit): unwrap the Options into a plain Vec<usize>.
        let level_to_col: Vec<usize> =
            level_to_col.into_iter().map(|c| c.expect("every trie level must have a column")).collect();

        let arity = shape.len();
        let mut root = Trie::Node(TrieMap::default());
        // For the N == 0 case (an atom with no variables, like R(2)) there is no root
        // Node; we only need to know whether any row survived the filters.
        let mut any_row = false;

        // This interprets filters & level_to_col in the inner loop. Seems to perform well
        // enough on simple benchmarks. If it becomes a problem, redesign to lift as much
        // interpretation as possible out of the loop and see if that fixes it.
        for row in db.rows(rel) {
            debug_assert!(row.len() == arity, "row arity {} != shape arity {arity}", row.len());

            // Apply filters; discard the row on any failure.
            let keep = filters.iter().all(|(col, colshape)| match colshape {
                IndexColumnShape::EqConst(v) => row[*col] == *v,
                IndexColumnShape::EqColumn(j) => row[*col] == row[*j],
                IndexColumnShape::TrieLevel(_) => unreachable!("filters holds no TrieLevels"),
            });
            if !keep { continue }
            any_row = true;

            // Walk the surviving row's path into the trie, materializing intermediate
            // Nodes on the way down and a Leaf at the bottom.
            let mut node = &mut root;
            for (level, &col) in level_to_col.iter().enumerate() {
                let deepest = level == n_levels - 1;
                match node {
                    Trie::Node(map) => {
                        node = map.entry(row[col]).or_insert_with(|| {
                            if deepest { Trie::Leaf } else { Trie::Node(TrieMap::default()) }
                        });
                    }
                    Trie::Leaf => unreachable!("only the deepest level holds Leaves"),
                }
            }
        }

        if !any_row { None }
        else if n_levels == 0 { Some(Trie::Leaf) }
        else { Some(root) }
    }
}

// ---------- AN ALTERNATIVE APPROACH TO CONSTANTS & VARIABLE DUPLICATION ----------
//
// Constants can be handled by rewriting the query to use singleton relations:
//
//     R(x,2) ---> R(x,y) is2(y)
//
// Where is2 = {2}. Singleton relations are easily materialized.
//
// Variable duplication can be handled with a non-materializable equality relation:
//
//     R(x,x) --> R(x,y) equal(x,y)
//
// We want to handle non-materializable relations eventually, so once we do, we might be
// able to simplify this code.

// ---------- ON TRIE INDEX SHARING ----------
//
// For now, we only re-use trie indexes when they have the same IndexShape. For instance,
// if the variable order is x,y,z then the atoms R(x,y), R(y,z), R(x,z) use the same
// index, but R(y,x) will need a different one.
//
// In principle we can do more interesting re-use, for instance, R(x,y) and R(2,x) and
// R(x,x) can use the same index. It is more obvious how to do this using the "alternative
// approach" of desugaring constants and variable re-use into separate atoms.


// ---------- WCOJ QUERY PLANS ----------
struct QueryPlan<'a> {
    // TODO: rewrite to Vec<Option<&'a Trie>> because indexes can be empty. in this case
    // there are no query results; we should handle this in QueryPlan::execute_dfs().
    tries: Vec<&'a Trie>, // one trie per atom.
    levels: Vec<Vec<usize>>,      // one level per variable
    // Some of the trie pointers may be identical if atoms share indexes.
}

// A QueryPlan has one `tries` entry for each atom in the query and one level for each
// variable. The index entries may happen to be duplicates of the same index; that's
// deliberate: when we execute the plan, we're going to maintain some distinct mutable
// state corresponding to each entry.
//
// levels[i]: bounds for variable i.
// levels[i][j]: the trie which we should use to bound this variable.
// Let t = tries[k] be a trie and d be its depth. Then `k` should occur exactly `d`
// times in `levels`, each occurrence corresponding to one level of `t`.
//
// Example: Consider the query
//
//     E(x,y) E(y,z) E(z,x) with variable order x,y,z
//
// We'll need two trie indexes, fwd(x,y) = E(x,y) and bwd(y,x) = E(x,y). You can think of
// this as rewriting the query so that every atom has the same variable order:
//
//     fwd(x,y) fwd(y,z) bwd(x,z)
//
// Then this becomes:
//
//     QueryPlan {
//         tries: [&fwd, &fwd, &bwd],
//         levels: [[0,2],      // x ← fwd    ∩ bwd    = {x : ∃y,z. E(x,y) ∧ E(z,x)}
//                  [0,1],      // y ← fwd[x] ∩ fwd    = {y : E(x,y) ∧ ∃z. E(y,z)}
//                  [1,2]],     // z ← fwd[y] ∩ bwd[x] = {z : E(y,z) ∧ E(z,x) }
//     }

impl<'a> QueryPlan<'a> {
    // Execute via depth-first backtracking.
    fn execute_dfs<F>(&self, mut f: F) where F: FnMut(&[Value]) {
        if self.levels.len() == 0 { // TODO: add a test for this case.
            f(&[]);
            return;
        }
        QueryDfsState {
            tries: self.tries.iter().map(|&t| match t {
                Trie::Node(map) => map,
                Trie::Leaf => unreachable!(),
            }).collect(),
            levels: &self.levels,
            prefix: Vec::with_capacity(self.levels.len()),
            children: Vec::new(),
            saved: Vec::new(),
            callback: f,
        }.execute(0)
    }
}

struct QueryDfsState<'a, F> {
    callback: F,
    tries: Vec<&'a TrieMap>,    // the current node in each trie that we're investigating.
    levels: &'a Vec<Vec<usize>>,
    prefix: Vec<Value>,      // partial solution: prefix[i] = value of ith variable.
    children: Vec<&'a Trie>, // scratch buffer used to avoid per-call allocation.
    // Trie node stack. When entering a level we push the current node of each
    // trie in that level; on leaving we restore them.
    saved: Vec<&'a TrieMap>,
}

impl<'a, F: FnMut(&[Value])> QueryDfsState<'a, F> {
    fn execute(&mut self, level_idx: usize) {
        let level: &Vec<usize> = &self.levels[level_idx];
        // Snapshot the current node of each trie in this level onto the `saved` stack so we
        // can restore them when we're done; `mark` is where this level's slice begins.
        let mark = self.saved.len();
        for &trie_idx in level { self.saved.push(self.tries[trie_idx]); }
        // The proposer is the trie in this level with the fewest children. We read each
        // level trie's map off the `saved` stack (positions mark..mark+width).
        let width = level.len();
        let proposer_pos: usize = (0..width)
            .min_by_key(|&pos| self.saved[mark + pos].len())
            .unwrap();

        let proposer_map = self.saved[mark + proposer_pos];
        'keys: for (key, child) in proposer_map {
            self.children.clear();
            // Look up this key in each trie at this level. If any trie lacks this key,
            // skip to the next key.
            for pos in 0..width {
                if pos == proposer_pos { self.children.push(child); continue; }
                match self.saved[mark + pos].get(key) {
                    Some(child) => self.children.push(child),
                    None => continue 'keys,
                }
            }
            // Write the children into `self.tries` and recurse. A Leaf child bottoms out
            // here and is never read again, so we skip it.
            for (pos, &trie_idx) in level.iter().enumerate() {
                if let Trie::Node(map) = self.children[pos] { self.tries[trie_idx] = map; }
            }
            self.recur(*key, level_idx + 1)
        }

        // Restore every trie in this level to the parent node the caller left it at, then
        // pop this level's slice off the `saved` stack.
        for (pos, &trie_idx) in level.iter().enumerate() {
            self.tries[trie_idx] = self.saved[mark + pos];
        }
        self.saved.truncate(mark);
    }

    #[inline]
    fn recur(&mut self, next: Value, level_idx: usize) {
        self.prefix.push(next);
        if level_idx == self.levels.len() {
            (self.callback)(self.prefix.as_slice());
        } else {
            self.execute(level_idx);
        }
        let popped = self.prefix.pop();
        debug_assert!(popped == Some(next));
    }
}


// ---------- BENCHMARKS ----------
fn main() {
    let datasets: Vec<&'static str> = vec![
        "ca-GrQc.txt",          // 14k undirected edges -> 48k undirected triangles
        "wiki-Vote.txt",        // 100k -> 600k
        "email-Enron.txt",      // 184k -> 700k
        "soc-Slashdot0811.txt", // 470k -> 550k
        "cit-HepTh.txt",        // 350k -> 1.5m
        "soc-Epinions1.txt",    // 400k -> 1.6m
        // "twitter_combined.txt", // 1.3m -> 13m          ~2s to run
        // "soc-LiveJournal1.txt", // 43m  -> 285m         ~2min to run!
    ];

    // With FxHash, WCO underperforms non-WCO on these (except LiveJournal1).
    // With SipHash (Rust default), it beats non-WCO except on ca-GrQc.
    // So they're competitive, but the non-WCO does more hash probes.
    println!("========== UNDIRECTED TRIANGLE BENCHMARKS ==========");
    for &name in &datasets {
        tests::snap_triangles_undirected(name, None);
    }

    // // These mostly, but not always, generate many more results. NB. each directed
    // // triangle is counted 3x (for its 3 rotations), except for self-triangles (x->x->x).
    // println!("========== DIRECTED TRIANGLE BENCHMARKS ==========");
    // for &name in &datasets {
    //     tests::snap_triangles_directed(name, None);
    // }
}


// ============================================================================
// ========= TESTS, BENCHMARKS, & HELPERS (mostly claude-generated)  ==========
// ============================================================================
//
// Run the tests with:
//
//     cargo test --example join-v2
//     cargo test --example join-v2 -- --nocapture   # to see the SNAP timing output
//
// Since query planning / variable-order selection don't exist yet, each test
// hand-builds the trie indexes and the `QueryPlan` (indexes + levels) that a
// planner would eventually produce, then checks `Trie::build` and
// `QueryPlan::execute_dfs` against a brute-force computation over small data.
#[allow(unused_imports)]
mod tests {
    use super::*;
    use super::IndexColumnShape::{TrieLevel, EqColumn, EqConst};
    use std::collections::{HashMap, HashSet};

    // ---- Loading SNAP graph datasets ----
    fn load_edges_from<R: std::io::Read>(source: R, max_edges: Option<usize>) -> Vec<(usize, usize)> {
        if let Some(n) = max_edges {
            print_flush!("Reading at most {n} edges.");
        } else {
            print_flush!("Reading all edges.");
        }
        use std::io::{BufRead, BufReader};
        let file = BufReader::new(source);
        let mut edges: Vec<(usize, usize)> = Vec::new();
        for readline in file.lines() {
            if max_edges.is_some_and(|n| n <= edges.len()) { break }
            let line = readline.expect("read error");
            if line.is_empty() { continue }
            if line.starts_with('#') { continue }
            let mut elts = line[..].split_whitespace();
            let v: usize = elts.next().unwrap().parse().expect("malformed src");
            let u: usize = elts.next().unwrap().parse().expect("malformed dst");
            edges.push((v,u));
        }
        print_flush!(" Got {} edges", edges.len());
        if edges.is_sorted() {
            print_flush!(", already sorted.");
        } else {
            print_flush!(", sorting...");
            edges.sort_unstable();
            print_flush!(" done.");
        }
        // Get rid of dupes. This ensures our trie-based WCOJs (which dedup implicitly) will
        // produce the same # of results as any other approach (which might not).
        print_flush!(" Deduping...");
        let before = edges.len();
        edges.dedup();
        if edges.len() == before {
            println!(" no dupes.");
        } else {
            println!(" deduped {} -> {}.", before, edges.len());
        }
        return edges;
    }

    // ---- A trivial in-memory Database backed by Vecs. ----
    struct VecDb {
        // name -> (arity, rows)
        rels: HashMap<&'static str, (usize, Vec<Vec<Value>>)>,
    }

    impl VecDb {
        fn new() -> Self { VecDb { rels: HashMap::new() } }

        // Builder-style: add a relation. Panics if a row's width != arity.
        fn rel(mut self, name: &'static str, arity: usize, rows: Vec<Vec<Value>>) -> Self {
            for row in &rows { assert_eq!(row.len(), arity, "bad row width in {name}"); }
            self.rels.insert(name, (arity, rows));
            self
        }
    }

    impl Database for VecDb {
        type RelId = &'static str;
        fn arity(&self, r: &'static str) -> usize { self.rels[r].0 }
        fn count(&self, r: &'static str) -> usize { self.rels[r].1.len() }
        fn rows(&self, r: &'static str) -> impl Iterator<Item = &[Value]> {
            self.rels[r].1.iter().map(|row| row.as_slice())
        }
    }

    // ---- Small helpers. ----

    // Sorted keys of a trie node's map.
    fn keys(node: &Trie) -> Vec<Value> {
        match node {
            Trie::Node(map) => { let mut k: Vec<Value> = map.keys().copied().collect(); k.sort(); k }
            Trie::Leaf => panic!("expected a Trie::Node, got a Leaf"),
        }
    }

    // Child of a node under `key` (panics if absent or if node is a Leaf).
    fn child<'a>(node: &'a Trie, key: Value) -> &'a Trie {
        match node {
            Trie::Node(map) => map.get(&key).expect("missing key"),
            Trie::Leaf => panic!("expected a Trie::Node, got a Leaf"),
        }
    }

    fn is_leaf(node: &Trie) -> bool { matches!(node, Trie::Leaf) }

    // Run a plan and return its output rows in sorted order.
    fn run_plan(plan: &QueryPlan) -> Vec<Vec<Value>> {
        let mut out: Vec<Vec<Value>> = Vec::new();
        let mut counter: usize = 0;
        plan.execute_dfs(|row| {
            out.push(row.to_vec());
            counter += 1;
            if counter % 1_000_000 == 0 {
                println!("found {:2} million results!", counter / 1_000_000);
            }
        });
        normalize(out)
    }

    fn normalize(mut v: Vec<Vec<Value>>) -> Vec<Vec<Value>> {
        v.sort_unstable();
        v
    }

    // Build a Database with a single binary relation "E" from an edge list.
    fn edge_db(edges: &[(Value, Value)]) -> VecDb {
        let rows: Vec<Vec<Value>> = edges.iter().map(|&(a, b)| vec![a, b]).collect();
        VecDb::new().rel("E", 2, rows)
    }

    // ---- Test 1: Trie::build across all IndexColumnShape kinds. ----
    //
    // Exercises: multi-level tries, a non-identity permutation shape, the
    // EqColumn filter (R(x,x)), empty results (-> None), and the zero-level
    // EqConst path (-> Some(Leaf) / None).
    #[test]
    fn test_trie_build() {
        let db = VecDb::new()
            .rel("E", 2, vec![vec![0, 1], vec![0, 2], vec![1, 2]])
            .rel("R", 2, vec![vec![0, 0], vec![1, 2], vec![3, 3], vec![2, 2]])
            .rel("S", 2, vec![vec![0, 1], vec![1, 0]])
            .rel("T", 1, vec![vec![5], vec![6]]);

        // Forward index E(x,y): level 0 = col 0, level 1 = col 1.
        let fwd = Trie::build(&db, "E", &vec![TrieLevel(0), TrieLevel(1)]).unwrap();
        assert_eq!(keys(&fwd), vec![0, 1]);
        assert_eq!(keys(child(&fwd, 0)), vec![1, 2]);
        assert_eq!(keys(child(&fwd, 1)), vec![2]);
        assert!(is_leaf(child(child(&fwd, 0), 1)));

        // Backward index E(x,y) with a *swapped* shape: level 0 = col 1 (the
        // destination), level 1 = col 0 (the source). So top-level keys are the
        // set of destinations.
        let bwd = Trie::build(&db, "E", &vec![TrieLevel(1), TrieLevel(0)]).unwrap();
        assert_eq!(keys(&bwd), vec![1, 2]);       // destinations
        assert_eq!(keys(child(&bwd, 2)), vec![0, 1]); // sources of edges into 2

        // R(x,x): EqColumn(0) keeps only rows where col1 == col0; depth-1 trie.
        let diag = Trie::build(&db, "R", &vec![TrieLevel(0), EqColumn(0)]).unwrap();
        assert_eq!(keys(&diag), vec![0, 2, 3]);
        assert!(is_leaf(child(&diag, 0)));

        // S has no diagonal rows, so R(x,x)-style build over S is empty -> None.
        assert!(Trie::build(&db, "S", &vec![TrieLevel(0), EqColumn(0)]).is_none());

        // Zero-level (fully constant) atom via EqConst: Some(Leaf) iff a match exists.
        match Trie::build(&db, "T", &vec![EqConst(5)]) {
            Some(Trie::Leaf) => {}
            other => panic!("T(5) should build Some(Leaf), got {:?}", other.is_some()),
        }
        assert!(Trie::build(&db, "T", &vec![EqConst(9)]).is_none());
    }

    // ---- Test 2: triangle query E(x,y) E(y,z) E(z,x), order x,y,z. ----
    //
    // This is the worked example in the QueryPlan doc comment: the canonical
    // worst-case-optimal-join workload. Checked against a brute-force scan.
    #[test]
    fn test_triangle_query() {
        let edges: Vec<(Value, Value)> = vec![
            (0, 1), (1, 2), (2, 0),   // a directed 3-cycle
            (0, 2), (2, 1), (1, 0),   // and its reverse
            (1, 3), (3, 1),           // extra edges, not in any triangle here
        ];
        let db = edge_db(&edges);
        // fwd = E indexed (source, dest); bwd = E indexed (dest, source). Rewritten
        // atoms: fwd(x,y) fwd(y,z) bwd(x,z).
        let fwd = Trie::build(&db, "E", &vec![TrieLevel(0), TrieLevel(1)]).unwrap();
        let bwd = Trie::build(&db, "E", &vec![TrieLevel(1), TrieLevel(0)]).unwrap();
        let plan = QueryPlan {
            tries: vec![&fwd, &fwd, &bwd],
            levels: vec![vec![0, 2], vec![0, 1], vec![1, 2]],
        };
        let got = run_plan(&plan);
        let want = binary_triangles_directed(&edges);

        assert!(!want.is_empty(), "test data should contain triangles");
        assert_eq!(got, want, "triangle join mismatch");
    }

    // The directed triangle query E(x,y) E(y,z) E(z,x), via a binary join for E(x,y) E(y,z)
    // followed by a hash-filter on E(z,x), then sorted. Used to cross-check the WCOJ plan,
    // both in the unit test above and in the SNAP benchmark below.
    fn binary_triangles_directed(edges: &[(Value, Value)]) -> Vec<Vec<Value>> {
        let edge_set: Set<(Value, Value)> = edges.iter().copied().collect();
        let mut out: Map<Value, Vec<Value>> = Map::default();
        for &(a, b) in edges { out.entry(a).or_default().push(b); }
        let mut want: Vec<Vec<Value>> = Vec::new();
        for &(x, y) in edges {
            if let Some(zs) = out.get(&y) {
                for &z in zs {
                    if edge_set.contains(&(z, x)) { want.push(vec![x, y, z]); }
                }
            }
        }
        normalize(want)
    }

    // Normalize an edge list into an undirected simple graph oriented low -> high: reorient
    // every edge so src < dst, drop self-loops, then sort & dedup. In the result every edge
    // (a, b) has a < b, so an undirected triangle {a < b < c} appears uniquely as the three
    // edges a->b, b->c, a->c — which is what lets the query below count each triangle once.
    fn to_low_high(edges: &[(Value, Value)]) -> Vec<(Value, Value)> {
        let mut v: Vec<(Value, Value)> = edges.iter()
            .filter(|&&(a, b)| a != b)
            .map(|&(a, b)| if a < b { (a, b) } else { (b, a) })
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    // Finds undirected triangles over a low->high edge list: all {a < b < c} with edges
    // a->b, b->c, a->c. Uses a binary join; sorts results.
    fn binary_triangles_undirected(edges: &[(Value, Value)]) -> Vec<Vec<Value>> {
        let edge_set: Set<(Value, Value)> = edges.iter().copied().collect();
        let mut out: Map<Value, Vec<Value>> = Map::default();
        for &(a, b) in edges { out.entry(a).or_default().push(b); }
        let mut want: Vec<Vec<Value>> = Vec::new();
        for &(a, b) in edges {
            if let Some(cs) = out.get(&b) {
                for &c in cs {
                    if edge_set.contains(&(a, c)) { want.push(vec![a, b, c]); }
                }
            }
        }
        normalize(want)
    }

    // ---- Test 3: two-atom path query E(x,y) E(y,z), order x,y,z. ----
    //
    // A trie shared by two atom-entries (both use `fwd`), so it exercises the
    // save/restore of a trie that participates in multiple levels.
    #[test]
    fn test_path_query() {
        let edges: Vec<(Value, Value)> = vec![
            (0, 1), (1, 2), (1, 3), (2, 3), (3, 0),
        ];
        let db = edge_db(&edges);
        let fwd = Trie::build(&db, "E", &vec![TrieLevel(0), TrieLevel(1)]).unwrap();

        // levels: x <- entry0; y <- entry0 ∩ entry1; z <- entry1.
        let plan = QueryPlan {
            tries: vec![&fwd, &fwd],
            levels: vec![vec![0], vec![0, 1], vec![1]],
        };
        let got = run_plan(&plan);

        let mut want: Vec<Vec<Value>> = Vec::new();
        for &(x, y) in &edges {
            for &(y2, z) in &edges {
                if y2 == y { want.push(vec![x, y, z]); }
            }
        }
        let want = normalize(want);

        assert!(!want.is_empty(), "test data should contain 2-paths");
        assert_eq!(got, want, "path join mismatch");
    }

    // ---- Test 4: single self-join atom R(x,x), order x. ----
    //
    // Exercises the EqColumn trie inside execute_dfs (a depth-1 join whose only
    // trie came from a variable-reuse shape).
    #[test]
    fn test_self_loop_query() {
        let db = VecDb::new().rel(
            "R", 2,
            vec![vec![0, 0], vec![1, 1], vec![2, 3], vec![4, 4], vec![5, 6]],
        );
        let diag = Trie::build(&db, "R", &vec![TrieLevel(0), EqColumn(0)]).unwrap();
        let plan = QueryPlan { tries: vec![&diag], levels: vec![vec![0]] };
        let got = run_plan(&plan);
        assert_eq!(got, vec![vec![0], vec![1], vec![4]], "self-loop mismatch");
    }

    #[test]
    fn test_undirected_triangle_query() {
        // Raw edges with mixed orientation, a self-loop, and a duplicate — all normalized away.
        let raw: Vec<(Value, Value)> = vec![
            (1, 0), (1, 2), (2, 0),   // triangle {0,1,2}
            (0, 3), (3, 4), (4, 0),   // triangle {0,3,4}
            (2, 2),                   // self-loop -> dropped
            (0, 1),                   // duplicate of (1,0) after reorientation
        ];
        let edges = to_low_high(&raw);
        assert_eq!(edges, vec![(0,1),(0,2),(0,3),(0,4),(1,2),(3,4)]);

        let db = edge_db(&edges);
        let fwd = Trie::build(&db, "E", &vec![TrieLevel(0), TrieLevel(1)]).unwrap();
        let plan = QueryPlan {
            tries: vec![&fwd, &fwd, &fwd],
            levels: vec![vec![0, 2], vec![0, 1], vec![1, 2]],
        };
        let got = run_plan(&plan);
        let want = binary_triangles_undirected(&edges);
        assert_eq!(got, want, "undirected join vs brute force");
        assert_eq!(got, vec![vec![0, 1, 2], vec![0, 3, 4]], "expected exactly two triangles");
    }

    fn snap_load(dataset: &str, max_edges: Option<usize>) -> Vec<(usize, usize)> {
        use std::fs::File;
        let path = format!("{}/data/{dataset}", env!("CARGO_MANIFEST_DIR"));
        let file = File::open(&path).expect("could not open data file");
        println!("{dataset}: loading from {path}");
        // load_edges_from already sorts.
        load_edges_from(file, max_edges)
    }

    // ---- Test 5: triangle query on a real SNAP dataset. ----
    //
    // Loads (a prefix of) the named dataset from data/ and runs the same triangle query
    // as test 2, cross-checked against brute force. `max_edges` caps how much of the file
    // we read so we can start small and scale up; None means "the whole file". The crate
    // directory is resolved at compile time, so it works regardless of the working
    // directory; if the file is missing the test is skipped, not failed.
    pub fn snap_triangles_directed(dataset: &str, max_edges: Option<usize>) {
        let edges = snap_load(dataset, max_edges);
        let db = edge_db(&edges);

        // WCOJ phase 1: build the trie indexes.
        let wcoj_start = Instant::now();
        let fwd = Trie::build(&db, "E", &vec![TrieLevel(0), TrieLevel(1)]).unwrap();
        let bwd = Trie::build(&db, "E", &vec![TrieLevel(1), TrieLevel(0)]).unwrap();
        let build_time = wcoj_start.elapsed();

        // WCOJ phase 2: execute the join, materializing + sorting the results just like the
        // brute force does, so the two are compared on equal terms.
        let plan = QueryPlan {
            tries: vec![&fwd, &fwd, &bwd],
            levels: vec![vec![0, 2], vec![0, 1], vec![1, 2]],
        };
        let t = Instant::now();
        let got = run_plan(&plan);
        let exec_time = t.elapsed();
        let total_time = wcoj_start.elapsed();

        let t = Instant::now();
        let want = binary_triangles_directed(&edges);
        let brute_time = t.elapsed();

        println!(
            "{dataset}: {} undirected edges -> {} triangles
  wcoj build    {:>9.2?}
  wcoj execute  {:>9.2?}
  wcoj total    {:>9.2?}    found {:8} triangles
  2-edge-filter {:>9.2?}    found {:8} triangles
",
            edges.len(), got.len(),
            build_time,
            exec_time,
            total_time, got.len(),
            brute_time, want.len(),
        );

        // There are too many triangles to print on mismatch, so just compare counts first
        // (a nicer message than a full set diff) and then the full sets.
        assert_eq!(got.len(), want.len(), "triangle count mismatch");
        assert!(got == want, "triangle set mismatch");
    }

    // ---- Test 6: undirected triangle count (matches SNAP's published figures). ----
    //
    // Reorient edges low->high and dedup, so each undirected triangle {a<b<c} shows up as
    // a->b, b->c, a->c exactly once. The query is therefore E(x,y) E(y,z) E(x,z) (note the
    // last atom, vs E(z,x) for directed 3-cycles), order x,y,z.
    pub fn snap_triangles_undirected(dataset: &str, max_edges: Option<usize>) {
        let raw = snap_load(dataset, max_edges);
        let edges = to_low_high(&raw);
        let db = edge_db(&edges);

        let wcoj_start = Instant::now();
        let fwd = Trie::build(&db, "E", &vec![TrieLevel(0), TrieLevel(1)]).unwrap();
        let build_time = wcoj_start.elapsed();

        let plan = QueryPlan {
            tries: vec![&fwd, &fwd, &fwd],
            levels: vec![vec![0, 2], vec![0, 1], vec![1, 2]],
        };
        let t = Instant::now();
        let got = run_plan(&plan);
        let exec_time = t.elapsed();
        let total_time = wcoj_start.elapsed();

        let t = Instant::now();
        let want = binary_triangles_undirected(&edges);
        let brute_time = t.elapsed();

        println!(
            "{dataset}: {} undirected edges -> {} triangles
  wcoj build    {:>9.2?}
  wcoj execute  {:>9.2?}
  wcoj total    {:>9.2?}    found {:8} triangles
  2-edge-filter {:>9.2?}    found {:8} triangles
",
            edges.len(), got.len(),
            build_time,
            exec_time,
            total_time, got.len(),
            brute_time, want.len(),
        );

        assert_eq!(got.len(), want.len(), "undirected triangle count mismatch");
        assert!(got == want, "undirected triangle set mismatch");
    }
}
