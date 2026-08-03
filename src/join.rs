use std::hash::Hash;

use crate::hash::Map;

// ---------- NEXT THINGS TO IMPLEMENT ----------
//
// 0. Pick a variable order in a vaguely reasonable way.
// 0. Construct indexes given a query and a variable order.
// 0. Computational atoms?
// 0. Representing & chasing FDs.

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
//    Future problems for this approach: computation over interned values. For attribute
//    values that fit in a usize, this is not a problem. For strings etc, computation
//    atoms would need the ability to get the value for a given interned id, which is
//    fine. The real problem is if the computation produces a new value, one that isn't
//    interned. How do we represent this? We could intern it, but... what if it's only
//    used temporarily in the query result? Then we've expanded our intern table for no
//    purpose. I think the best approach is to use a "temporary" intern table that holds
//    only these intermediate results, but this is kinda complicated :(.
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
//
// 7. Decode the results by de-interning everything.


// ---------- DATABASES AND QUERIES ----------
//
// I'm assuming we intern everything up front. This makes things simpler than figuring out
// where to put tags to minimize tag-checking overhead.
pub type Value = usize;

// A database is something which has relations and can tabulate them.
//
// TODO: how do I incorporate functional dependency information here?
//
// ANSWER: simplest way is to let each relation declare a primary key, which all other
// keys are determined by. This is less general than full FDs but easier to represent and
// plan around and handles ACSet-type schemas.
pub trait Database {
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

pub struct Query<Db: Database, Var: Eq + Hash + Copy> {
    pub vars: Vec<Var>,
    pub atoms: Vec<Atom<Db::RelId, Var>>,
}

// TODO: how do we represent constants in atoms?
pub struct Atom<RelId, Var> {
    pub relation: RelId,
    pub vars: Vec<Var>,
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
pub enum Trie {
    Leaf,
    Node(TrieMap),
}
pub type TrieMap = Map<Value, Trie>;

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
// Unfortunately, while sorted vectors are simple and efficient and have much nicer memory
// access patterns than a nested-hashtable trie, they can't be updated in-place
// efficiently. So you'll need a B-tree, LSM-tree, or similar. These are quite a bit more
// complicated to implement and work with. (Dijkstralog contains an LSM implementation.)
//
// We can't directly use Rust stdlib's BTreeMap because it doesn't support the primitives
// needed for LFTJ: we need to be able to keep an internal iterator into the BTree that
// can efficiently "seek" forward toward an upper bound. Standard iterators don't do this.
// There's an experimental "Cursor" interface on BTrees available on Rust nightly (as of
// 2026-07-31) that gets partway there:
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

pub type IndexShape = Vec<IndexColumnShape>; // length = arity of relation
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum IndexColumnShape {
    // what to do with column i.
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
    pub fn build<Db: Database>(db: &Db, rel: Db::RelId, shape: &IndexShape) -> Option<Trie> {
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
pub struct QueryPlan<'a> {
    // TODO: rewrite to Vec<Option<&'a Trie>> because indexes can be empty. in this case
    // there are no query results; we should handle this in QueryPlan::execute_dfs().
    pub tries: Vec<&'a Trie>, // one trie per atom.
    pub levels: Vec<Vec<usize>>,  // one level per variable
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
    pub fn execute_dfs<F>(&self, f: F) where F: FnMut(&[Value]) {
        // TODO: add a 0-level query test case.
        let empty = Map::default();
        QueryDfsState {
            tries: self.tries.iter().map(|&t| match t {
                Trie::Node(map) => map,
                // We never read leaves but we need something to put here. We hit this
                // case on a query with a fully-constant atom eg R(2) and also
                // non-constant atoms eg T(x,y). TODO: test that hits this case.
                Trie::Leaf => &empty,
            }).collect(),
            levels: &self.levels,
            prefix: Vec::with_capacity(self.levels.len()),
            children: Vec::new(),
            saved: Vec::new(),
            callback: f,
        }.execute(0)
    }

    // Convenience wrapper used by tests and benchmarks.
    pub fn collect_dfs(&self) -> Vec<Vec<Value>> {
        let mut out: Vec<Vec<Value>> = Vec::new();
        self.execute_dfs(|row| out.push(row.to_vec()));
        out.sort_unstable();
        out
    }
}

struct QueryDfsState<'a, F> {
    callback: F,
    levels: &'a Vec<Vec<usize>>,
    // Partial solution: prefix[i] = value of ith variable.
    prefix: Vec<Value>,
    // For each atom, the data of the node in the corresponding trie that we're currently
    // at. (If the trie bottoms-out, it doesn't matter what we store here - we don't read
    // Leafs.)
    tries: Vec<&'a TrieMap>,
    // Trie node stack. When entering a level we push the current node of each
    // trie in that level; on leaving we restore them.
    saved: Vec<&'a TrieMap>,
    // Scratch buffer used to avoid per-call allocation.
    children: Vec<&'a Trie>,
}

impl<'a, F: FnMut(&[Value])> QueryDfsState<'a, F> {
    fn execute(&mut self, level_idx: usize) {
        if level_idx == self.levels.len() {
            (self.callback)(&self.prefix);
            return;
        }
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
            .expect("Empty level - every query variable must be used in some atom!");

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

            // We've found a match! Write the children into `self.tries` and recurse.
            self.prefix.push(*key);
            for (pos, &trie_idx) in level.iter().enumerate() {
                // A Leaf child bottoms out here and is never read again, so we skip it.
                if let Trie::Node(map) = self.children[pos] { self.tries[trie_idx] = map; }
            }
            self.execute(level_idx + 1);
            let popped = self.prefix.pop();
            debug_assert!(popped == Some(*key));
        }

        // Restore every trie in this level to the parent node the caller left it at, then
        // pop this level's slice off the `saved` stack.
        for (pos, &trie_idx) in level.iter().enumerate() {
            self.tries[trie_idx] = self.saved[mark + pos];
        }
        self.saved.truncate(mark);
    }
}


// ---------- UNIT TESTS (Claude-generated) ----------
//
// Run Trie::build across all IndexColumnShape kinds. This pokes at the trie's
// representation directly, so lives here rather than in the integration tests. Exercises:
// multi-level tries, a non-identity permutation shape, the EqColumn filter (R(x,x)),
// empty results (-> None), and the zero-level EqConst path (-> Some(Leaf) / None).
#[cfg(test)]
mod tests {
    use super::IndexColumnShape::{EqColumn, EqConst, TrieLevel};
    use super::*;
    use crate::vec_db::VecDb;

    // Sorted keys of a trie node's map.
    fn keys(node: &Trie) -> Vec<Value> {
        match node {
            Trie::Node(map) => { let mut k: Vec<Value> = map.keys().copied().collect(); k.sort(); k }
            Trie::Leaf => panic!("expected a Trie::Node, got a Leaf"),
        }
    }

    // Child of a node under `key` (panics if absent or if node is a Leaf).
    fn child(node: &Trie, key: Value) -> &Trie {
        match node {
            Trie::Node(map) => map.get(&key).expect("missing key"),
            Trie::Leaf => panic!("expected a Trie::Node, got a Leaf"),
        }
    }

    fn is_leaf(node: &Trie) -> bool { matches!(node, Trie::Leaf) }

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
}
