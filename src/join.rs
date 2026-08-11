use std::fmt::{self, Debug};
use std::hash::Hash;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::Value;
use crate::ValueType;
use crate::hash::Map;
use crate::op::Operator;

// ---------- NEXT THINGS TO IMPLEMENT ----------
//
// 0. Pick a variable order in a vaguely reasonable way.
//    heuristics to start with:
//    - join keys first
//    - connectedness (pick keys that are constrained by previous ones)
//    I think I built something like this in Racket once? go rummage around for it.
//
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
//    DONE, see Query::plan, QueryPlan::{build_indexes, bind}
//
// 6. Execute query using the indexes.
//    DONE: see ExecutableQuery::execute_dfs
//    there's also a tentative bfs version in join_bfs.rs.
//
// 7. Decode the results by de-interning everything.


// ---------- DATABASES AND QUERIES ----------
// A database is something which has relations and can tabulate them.
//
// TODO: how do I incorporate functional dependency information here?
//
// ANSWER: simplest way is to let each relation declare a primary key, which all other
// keys are determined by. This is less general than full FDs but easier to represent and
// plan around and handles ACSet-type schemas.
pub trait Database {
    // TODO: separate into Schema and Database.
    // Schema should be a struct with arity & FDs for each Rel. Map<Rel, RelInfo>?
    // Database trait should have count, rows and schema() -> Rc<Schema>.
    type Rel: Eq + Hash + Clone; // relation identifier.
    fn arity(&self, r: Self::Rel) -> usize;
    fn count(&self, r: Self::Rel) -> usize;
    // This assumes a row-oriented representation. for a columnar representation, maybe
    // Item = &[&Value], a slice of pointers to entries in each column?
    fn rows(&self, r: Self::Rel) -> impl Iterator<Item = &[Value]>;
}

// Var, Rel, Op stand for variable, relation, and computational operator identifiers.
// (See also `Operator` trait, below.)
pub struct Query<Var, Rel, Op = Rc<dyn Operator>> {
    pub vars: Vec<Var>,
    pub atoms: Vec<Atom<Rel, Var>>,      // relational atoms
    pub operators: Vec<Atom<Op, Var>>,   // computational atoms (operators)
    // We separate relational atoms from operators because the planner & execution
    // engine treat them differently. It's possible we could unify them with a redesign of
    // what query plans look like, but it doesn't seem obvious how. Differences:
    //
    // Atoms get tries built for them; operators don't.
    //
    // Atoms get consulted on every variable they touch; operators, for now, only on the
    // last one (see "Limitations of this trait for operators" below).
    //
    // Once we have incremental evaluation, relations will have derivatives/delta versions, but
    // operators don't change so they won't.
    //
    // Because we separate atoms from operators, we only pay dispatch overhead for
    // operators; queries without operators don't pay.
}

// An atom applies its predicate `pred` to `vars`. A relational atom (Atom<Rel, Var>) matches
// rows of a database relation; a computational atom (Atom<Op, Var>) applies an Operator.
//
// Invariant for relational atoms: vars.len() == db.arity(pred).
//
// Invariant for computational atoms: vars.len() == pred.arity(). The first pred.input_arity()
// vars are inputs; if pred.has_output(), the last var is the output.
#[derive(Clone)]
pub struct Atom<Pred, Var> {
    pub pred: Pred,
    pub vars: Vec<Var>,
    // TODO: how do we represent constants in atoms?
}

// Shown as `pred(var1, ..., varN)`.
impl<Pred: Debug, Var: Debug> Debug for Atom<Pred, Var> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}(", self.pred)?;
        for (i, v) in self.vars.iter().enumerate() {
            if i > 0 { write!(f, ", ")? }
            write!(f, "{v:?}")?;
        }
        write!(f, ")")
    }
}

impl<Var: Eq+Hash+Clone, Rel: Eq+Hash+Clone, Op: Operator> Query<Var, Rel, Op> {
    // Panics if the query is malformed.
    pub fn self_check<Db: Database<Rel = Rel>>(&self, db: &Db)
        where Var: Debug, Rel: Debug
    {
        let mut declared: HashSet<&Var> = HashSet::new();
        for v in &self.vars {
            assert!(declared.insert(v), "duplicate query variable {v:?}");
        }

        for atom in &self.atoms {
            assert_eq!(atom.vars.len(), db.arity(atom.pred.clone()),
                       "atom {atom:?} has the wrong number of variables for its relation's arity");
            for v in &atom.vars {
                assert!(declared.contains(v), "atom {atom:?} uses {v:?}, which is not in query.vars");
            }
        }

        for atom in &self.operators {
            assert_eq!(atom.vars.len(), atom.pred.arity(),
                       "operator {atom:?} has the wrong number of variables for its arity");
            // TODO: support zero-variable operators; they're constant checks, so they
            // belong at the very start of the query rather than at any variable's level.
            assert!(!atom.vars.is_empty(),
                    "operator {atom:?} has zero variables; we don't know how to plan those yet");
            for v in &atom.vars {
                assert!(declared.contains(v),
                        "operator {atom:?} uses {v:?}, which is not in query.vars");
            }
        }

        // A query is grounded if all its vars are grounded.
        let grounded: HashSet<Var> = self.ground_vars().into_iter().collect();
        for v in &self.vars {
            assert!(grounded.contains(v), "query variable {v:?} is not grounded");
        }
    }

    // Returns the grounded vars in the order we find them. Panics if an operator's
    // vars.len() < its input_arity(); self_check() catches that first.
    pub fn ground_vars(&self) -> Vec<Var> {
        // An atom grounds all its variables. An operator grounds its output if its inputs
        // are grounded. By applying these rules to saturation we can find the grounded
        // variables.
        let mut found: Vec<Var> = Vec::new();
        let mut grounded: HashSet<Var> = HashSet::new();
        for atom in &self.atoms {
            for v in &atom.vars {
                if grounded.insert(v.clone()) { found.push(v.clone()) }
            }
        }
        // Saturate by rescanning the operators until nothing new shows up. A more
        // efficient approach would hashmap vars to which operators they touch instead of
        // scanning them all; probably queries are small enough this doesn't matter.
        let mut opers: Vec<&Atom<Op, Var>> = self.operators.iter()
            .filter(|atom| atom.pred.has_output()).collect();
        loop {
            let count = found.len();
            opers.retain(|atom| {
                let op = &atom.pred;
                let inputs = &atom.vars[..op.input_arity()];
                let output = &atom.vars[op.input_arity()];
                let fires = inputs.iter().all(|v| grounded.contains(v));
                if fires && grounded.insert(output.clone()) {
                    found.push(output.clone());
                }
                !fires
            });
            if found.len() == count { return found }
        }
    }
}


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
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IndexColumnShape {
    // what to do with column i.
    TrieLevel(usize), // TrieLevel(k) => becomes trie level k.
    EqConst(Value),   // EqConst(v)   => filter: equal to v, otherwise discard.
    EqColumn(usize),  // EqColumn(j)  => filter: equal to column j, otherwise discard.
}

// INVARIANT: If shape[i] = EqColumn(j), we should have j < i and shape[j] = TrieLevel(_).
// This ensures that shapes which denote equivalent indexes are equal on the nose.

// The index shape for an atom whose columns are `atom_vars`, given the variable order
// positions `order_pos`.
fn index_shape<Var>(order_pos: &HashMap<Var, usize>, atom_vars: &[Var]) -> IndexShape
where Var : Eq + Hash + Clone
{
    use IndexColumnShape::{EqColumn, TrieLevel};

    // first_col[v] = the first column where variable v appears.
    let mut first_col: HashMap<Var, usize> = HashMap::default();
    for (col, v) in atom_vars.iter().enumerate() {
        first_col.entry(v.clone()).or_insert(col);
    }

    // Sort atom's variables according to the variable order.
    let mut by_order: Vec<&Var> = first_col.keys().collect();
    by_order.sort_unstable_by_key(|v| {
        *order_pos.get(v).expect("every atom variable must appear in the variable order")
    });
    let level_of: HashMap<Var, usize> =
        by_order.iter().enumerate().map(|(lvl, &v)| (v.clone(), lvl)).collect();

    // Build the shape column by column: a variable's first occurrence becomes a TrieLevel;
    // a repeat becomes an EqColumn back-reference to its first column. (A constant would
    // become EqConst here, once Atom can carry constants.)
    atom_vars.iter().enumerate().map(|(col, v)| {
        if first_col[v] == col { TrieLevel(level_of[v]) } else { EqColumn(first_col[v]) }
    }).collect()
}

impl Trie {
    // Trie::build() returns None if the trie is empty. This is necessary to distinguish
    // between Some(Trie::Leaf()), a trie containing an single empty tuple, and None, an
    // empty trie.
    pub fn build<Db: Database>(db: &Db, rel: Db::Rel, shape: &IndexShape) -> Option<Trie> {
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
// For now, we only re-use trie indexes when they have the same relation & IndexShape. For
// instance, if the variable order is x,y,z then the atoms R(x,y), R(y,z), R(x,z) use the
// same index, but R(y,x) will need a different one.
//
// In principle we can do more interesting re-use, for instance, R(x,y) and R(2,x) and
// R(x,x) can use the same index. It is more obvious how to do this using the "alternative
// approach" of desugaring constants and variable re-use into separate atoms.
//
// TODO: explain that this will only share more indexes with the right variable order, and that
// the more restrictive trie indexes can actually be more efficient (at the cost of more
// indexing).


// ---------- QUERY PLANNING ----------
pub struct QueryPlan<Rel, Op> {
    // TODO: could add a `var_order: Vec<Var>` here to be more self-describing?
    //
    // Replaces query atoms by the shape of the index we need for them.
    pub atoms: Vec<(Rel, IndexShape)>,
    // One level per variable, in variable order. levels[i] describes what to do when we
    // bind variable i: which atoms constrain it, and which operators to consult.
    pub levels: Vec<Level<Op>>,
}

// Everything the executor needs to do at a single variable's level.
//
// We use Atom<Op, usize> to represent operator calls (proposer/filters); for such an atom, its
// atom.vars are indexes into the variable order.
#[derive(Clone)]
pub struct Level<Op> {          // TODO: rename to VarPlan?
    // An operator to propose a unique value for this variable, computed from its inputs
    // (its output is this variable). If None, we propose from the trie node with the
    // fewest children.
    //
    // Once we have FDs and may know statically that a relation will propose exactly one
    // value, this can become `enum Proposer { Atom(usize), Op(Atom<Op, usize>) }`.
    pub proposer: Option<Atom<Op, usize>>,
    // Operators that filter bindings.
    pub filters: Vec<Atom<Op, usize>>,
    // The indexes in QueryPlan.atoms of the atoms constraining this variable.
    pub atoms: Vec<usize>,
}

// For example, Query { vars: [x,y,z], atoms: [R(x,y), R(y,z)] }.plan([x,y,z]) yields:
// QueryPlan {
//     atoms: [(R, [TrieLevel(0), TrieLevel(1)]),
//             (R, [TrieLevel(0), TrieLevel(1)])],
//     levels: [Level { atoms: [0],    proposer: None, filters: [] },
//              Level { atoms: [0, 1], proposer: None, filters: [] },
//              Level { atoms: [1],    proposer: None, filters: [] }],
// }
//
// An operator, say `w = x + z` (inputs x,z; output w) with order [x,y,z,w], adds a fourth
// level with no trie atoms:
//
//              Level { atoms: [], proposer: Some(Atom { vars: [0,2,3] }), filters: [] }
//
// TODO: test Query::plan with a non-identity var order, eg [y,x,z]. (See tests/queries.rs
// for the identity-order and repeated-variable cases.)

impl<Var, Rel, Op> Query<Var, Rel, Op> where
    Var: Eq + Hash + Clone,
    Rel: Eq + Hash + Clone,
    Op: Operator + Clone,
{
    pub fn plan(&self, order: &[Var]) -> QueryPlan<Rel, Op> {
        // TODO: check that order is a permutation of the query variables.

        // Given a var order, we go through every atom & operator in the query and assign
        // it to the levels it ought to be in.
        let mut levels: Vec<Level<Op>> = (0..order.len())
            .map(|_| Level { atoms: Vec::new(), proposer: None, filters: Vec::new() })
            .collect();

        // order_pos[v] = position of variable v in the variable order.
        let order_pos: HashMap<Var, usize> =
            order.iter().enumerate().map(|(i, v)| (v.clone(), i)).collect();
        assert_eq!(order_pos.len(), order.len(), "variable order repeats a variable");

        // For each atom, compute its IndexShape and put it in the appropriate levels.
        let mut atoms: Vec<(Rel, IndexShape)> = Vec::with_capacity(self.atoms.len());
        for (atom_idx, atom) in self.atoms.iter().enumerate() {
            let shape = index_shape(&order_pos, &atom.vars);
            // Each variable gets one TrieLevel column, so this hits each var once.
            for (col, v) in atom.vars.iter().enumerate() {
                if matches!(shape[col], IndexColumnShape::TrieLevel(_)) {
                    levels[order_pos[v]].atoms.push(atom_idx);
                }
            }
            atoms.push((atom.pred.clone(), shape));
        }

        // For each operator, put it in the level of its last variable in the var order;
        // by then every other var it touches is bound. If this variable is the operator's
        // output, then make it the proposer for the level if there isn't one already;
        // otherwise, a filter.
        for atom in &self.operators {
            let op = &atom.pred;
            assert_eq!(atom.vars.len(), op.arity(), "operator vars don't match its arity");
            assert!(!atom.vars.is_empty(), "zero-variable operators are not supported");
            // Convert Vars into indexes into variable order.
            let atom = Atom {
                pred: op.clone(),
                vars: atom.vars.iter().map(|v| {
                    *order_pos.get(v)
                        .expect("every operator variable must appear in the variable order")
                }).collect()
            };
            // Assign to level of last variable in var order.
            let last_pos = *atom.vars.iter().max().unwrap();
            let level = &mut levels[last_pos];
            // Propose if the output is this level's variable and no proposer yet exists.
            if op.has_output()
                && last_pos == atom.vars[op.input_arity()]
                && level.proposer.is_none() {
                level.proposer = Some(atom);
            } else {
                level.filters.push(atom);
            }
        }

        // Every variable needs a proposer: at least one trie atom binds it, or an operator
        // proposes it. (Otherwise nothing generates candidate values for it.)
        for (pos, level) in levels.iter().enumerate() {
            assert!(!level.atoms.is_empty() || level.proposer.is_some(),
                "variable at order position {pos} has no proposer: no atom binds it and no \
                 operator proposes it");
        }

        QueryPlan { atoms, levels }
    }
}

// The built trie indexes, keyed by (relation, shape) so shared indexes are stored once.
// A None value means the index is empty.
pub type Indexes<Rel> = HashMap<(Rel, IndexShape), Option<Trie>>;

impl<Rel: Eq+Hash+Clone, Op> QueryPlan<Rel, Op> {
    pub fn build_indexes<Db: Database<Rel = Rel>>(&self, db: &Db) -> Indexes<Rel> {
        let mut indexes: Indexes<Rel> = HashMap::new();
        for (rel, shape) in &self.atoms {
            indexes
                .entry((rel.clone(), shape.clone()))
                .or_insert_with(|| Trie::build(db, rel.clone(), shape));
        }
        indexes
    }

    // Bind indexes to the plan for execution. Returns None if any atom's index is empty:
    // a join is a conjunction, so one empty index means the whole query is empty.
    //
    // TODO: this means a query can be empty in two ways - either None here, or by
    // yielding nothing in execute_dfs(). This is ugly from a consumer's point of view.
    // Either unify these paths somehow or provide a way of running a query that papers
    // over the difference.
    pub fn bind<'a>(&self, indexes: &'a Indexes<Rel>) -> Option<ExecutableQuery<'a, Op>>
        where Op: Clone
    {
        let mut tries: Vec<&'a Trie> = Vec::with_capacity(self.atoms.len());
        for key in &self.atoms {
            match indexes.get(key) {
                Some(Some(trie)) => tries.push(trie),
                Some(None) => return None, // empty index => empty query.
                None => panic!("index not built for an atom; call build_indexes first"),
            }
        }
        Some(ExecutableQuery { tries, levels: self.levels.clone() })
    }
}


// ---------- QUERY EXECUTION ----------
//
// ExecutableQuery is like QueryPlan but replaces (RelId, IndexShape) atoms with pointers
// to the actual trie indices. TODO: unify these into one struct with a parameter for the
// representation of atoms.
pub struct ExecutableQuery<'a, Op> {
    pub tries: Vec<&'a Trie>, // one trie per atom.
    pub levels: Vec<Level<Op>>,  // one level per variable
    // Some of the trie pointers may be identical if atoms share indexes.
}

// Example: Consider the query
//
//     E(x,y) E(y,z) E(z,x) with variable order x,y,z
//
// We'll need two trie indexes, fwd(x,y) = E(x,y) and bwd(y,x) = E(x,y). You can think of
// this as rewriting the query so that every atom has the same variable order:
//
//     fwd(x,y) fwd(y,z) bwd(x,z)
//
// Then this becomes (writing just the `atoms` of each level; no operators here):
//
//     ExecutableQuery {
//         tries: [&fwd, &fwd, &bwd],
//         levels[_].atoms: [[0,2],  // x ← fwd    ∩ bwd    = {x : ∃y,z. E(x,y) ∧ E(z,x)}
//                           [0,1],  // y ← fwd[x] ∩ fwd    = {y : E(x,y) ∧ ∃z. E(y,z)}
//                           [1,2]], // z ← fwd[y] ∩ bwd[x] = {z : E(y,z) ∧ E(z,x) }
//     }

// Our query execution strategy is depth-first traversal of the implicit result trie with
// one level for each var in variable order. This struct contains the state needed for
// this process.
struct QueryDfsState<'a, Op, F> {
    callback: F,
    levels: &'a [Level<Op>],
    // Partial solution: prefix[i] = value of ith variable.
    prefix: Vec<Value>,
    // For each atom, the data of the node in the corresponding trie that we're currently
    // at. (If the trie bottoms-out, it doesn't matter what we store here - we don't read
    // Leafs.)
    tries: Vec<&'a TrieMap>,
    // Trie node stack. When entering a level we push the current node of each
    // trie in that level; on leaving we restore them.
    saved: Vec<&'a TrieMap>,
    // Scratch buffers used to avoid repeated allocation:
    input_buf: Vec<Value>,      // values for input to an Operator
}

impl<'a, Op: Operator> ExecutableQuery<'a, Op> {
    // Execute via depth-first backtracking.
    pub fn execute_dfs<F>(&self, f: F) where F: FnMut(&[Value]) {
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
            saved: Vec::new(),
            input_buf: Vec::new(),
            callback: f,
        }.execute(0)
    }

    // Convenience wrappers used by tests and benchmarks.
    pub fn collect_dfs(&self) -> Vec<Vec<Value>> {
        let mut out: Vec<Vec<Value>> = Vec::new();
        self.execute_dfs(|row| out.push(row.to_vec()));
        out.sort_unstable();
        out
    }
    pub fn collect_dfs_untagged<V: ValueType + Ord>(&self) -> Vec<Vec<V>> {
        let mut out: Vec<Vec<V>> = Vec::new();
        self.execute_dfs(|row| out.push(row.iter().map(|x| x.untag()).collect()));
        out.sort_unstable();
        out
    }
}

impl<'a, Op: Operator, F: FnMut(&[Value])> QueryDfsState<'a, Op, F> {
    // Copy the given prefix positions into `input_buf`, in order.
    fn gather(&mut self, positions: &[usize]) {
        self.input_buf.clear();
        for &pos in positions { self.input_buf.push(self.prefix[pos]); }
    }

    // Run this level's filter operators.
    fn filters_pass(&mut self, filters: &[Atom<Op, usize>]) -> bool {
        for f in filters {
            self.gather(&f.vars);
            if !f.pred.check(&self.input_buf) { return false; }
        }
        true
    }

    fn set_trie(&mut self, trie_idx: usize, child: &'a Trie) {
        // If it's a Leaf we don't need to load it, as it will never be read again.
        if let Trie::Node(map) = child {
            self.tries[trie_idx] = map;
        }
    }

    fn execute(&mut self, level_idx: usize) {
        if level_idx == self.levels.len() {
            (self.callback)(&self.prefix);
            return;
        }
        let level: &'a Level<Op> = &self.levels[level_idx];
        // Snapshot the current node of each trie in this level onto the `saved` stack so
        // we can restore them when we're done; `mark` is where this level's slice begins.
        // We can now mutate self.tries[i] for any i ∈ level.atoms and not worry that
        // we'll screw things up for our callee. From this point on, at this level, we
        // never read from `self.tries`, only write to it.
        let mark = self.saved.len();
        for &trie_idx in &level.atoms { self.saved.push(self.tries[trie_idx]); }
        let width = level.atoms.len();

        match &level.proposer {
            // An operator proposes a single value for this variable; every trie atom in this
            // level (and every filter) is a checker.
            Some(prop) => {
                // The proposer's output is this level's variable; compute it from the inputs.
                debug_assert_eq!(prop.vars.last(), Some(&level_idx));
                self.gather(&prop.vars[..prop.vars.len() - 1]);
                if let Some(key) = prop.pred.compute(&self.input_buf) {
                    // proposer_pos is deliberately out of bounds so it doesn't compare
                    // equal to any real position.
                    let proposer_pos = level.atoms.len();
                    self.propose(level, level_idx, mark, proposer_pos, &key);
                }
            }
            // No designated proposer, so we choose dynamically: the trie with the fewest
            // children proposes keys, and we look up each key in each other trie for this
            // level. We find these tries in the `saved` stack (positions
            // mark..mark+width).
            None => {
                let proposer_pos: usize = (0..width)
                    .min_by_key(|&pos| self.saved[mark + pos].len())
                    .expect("no proposer at this level - the planner should have caught this");
                let proposer_map = self.saved[mark + proposer_pos];
                for (key, child) in proposer_map {
                    self.set_trie(level.atoms[proposer_pos], child);
                    self.propose(level, level_idx, mark, proposer_pos, key);
                }
            }
        }

        for (pos, &trie_idx) in level.atoms.iter().enumerate() {
            self.tries[trie_idx] = self.saved[mark + pos];
        }
        self.saved.truncate(mark);
    }

    // Proposes a given key to all atoms/tries at this level, recursing if it's present in
    // all & passes the level's filters.
    fn propose(
        &mut self,
        level: &'a Level<Op>,
        level_idx: usize,
        mark: usize,
        proposer_pos: usize, // proposer position, or level.atoms.len() if no proposer trie
        key: &Value,
    ) {
        // Look up `key` in each trie & descend into it if found.
        for pos in 0..level.atoms.len() {
            if proposer_pos == pos { continue }
            let Some(child) = self.saved[mark + pos].get(key) else { return };
            self.set_trie(level.atoms[pos], child);
        }
        self.prefix.push(*key);
        if self.filters_pass(&level.filters) {
            self.execute(level_idx + 1);
        }
        let popped = self.prefix.pop();
        debug_assert!(popped == Some(*key));
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
    fn child(node: &Trie, key: usize) -> &Trie {
        match node {
            Trie::Node(map) => map.get(&key.into()).expect("missing key"),
            Trie::Leaf => panic!("expected a Trie::Node, got a Leaf"),
        }
    }

    fn is_leaf(node: &Trie) -> bool { matches!(node, Trie::Leaf) }

    #[test]
    fn test_trie_build() {
        let db = VecDb::new()
            .rel("E", 2, vec![row![0, 1], row![0, 2], row![1, 2]])
            .rel("R", 2, vec![row![0, 0], row![1, 2], row![3, 3], row![2, 2]])
            .rel("S", 2, vec![row![0, 1], row![1, 0]])
            .rel("T", 1, vec![row![5], row![6]]);

        // Forward index E(x,y): level 0 = col 0, level 1 = col 1.
        let fwd = Trie::build(&db, "E", &vec![TrieLevel(0), TrieLevel(1)]).unwrap();
        assert_eq!(keys(&fwd), row![0, 1]);
        assert_eq!(keys(child(&fwd, 0)), row![1, 2]);
        assert_eq!(keys(child(&fwd, 1)), row![2]);
        assert!(is_leaf(child(child(&fwd, 0), 1)));

        // Backward index E(x,y) with a *swapped* shape: level 0 = col 1 (the
        // destination), level 1 = col 0 (the source). So top-level keys are the
        // set of destinations.
        let bwd = Trie::build(&db, "E", &vec![TrieLevel(1), TrieLevel(0)]).unwrap();
        assert_eq!(keys(&bwd), row![1, 2]);       // destinations
        assert_eq!(keys(child(&bwd, 2)), row![0, 1]); // sources of edges into 2

        // R(x,x): EqColumn(0) keeps only rows where col1 == col0; depth-1 trie.
        let diag = Trie::build(&db, "R", &vec![TrieLevel(0), EqColumn(0)]).unwrap();
        assert_eq!(keys(&diag), row![0, 2, 3]);
        assert!(is_leaf(child(&diag, 0)));

        // S has no diagonal rows, so R(x,x)-style build over S is empty -> None.
        assert!(Trie::build(&db, "S", &vec![TrieLevel(0), EqColumn(0)]).is_none());

        // Zero-level (fully constant) atom via EqConst: Some(Leaf) iff a match exists.
        match Trie::build(&db, "T", &vec![EqConst(5.into())]) {
            Some(Trie::Leaf) => {}
            other => panic!("T(5) should build Some(Leaf), got {:?}", other.is_some()),
        }
        assert!(Trie::build(&db, "T", &vec![EqConst(9.into())]).is_none());
    }
}
