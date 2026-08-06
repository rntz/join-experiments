use std::hash::Hash;
use std::collections::HashMap;
use std::rc::Rc;

use crate::hash::Map;

// ---------- NEXT THINGS TO IMPLEMENT ----------
//
// 0. Pick a variable order in a vaguely reasonable way.
//    heuristics to start with:
//    - join keys first
//    - connectedness (pick keys that are constrained by previous ones)
//    I think I built something like this in Racket once? go rummage around for it.
//
// 0. Computational atoms?
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

pub struct Query<Var, Rel, Op = Rc<dyn Operator>> {
    pub vars: Vec<Var>,
    pub atoms: Vec<Atom<Rel, Var>>,      // relational atoms
    pub operators: Vec<OpCall<Op, Var>>, // computational operators
    // We separate relational Atoms from Operators here because the planner & execution
    // engine treat them differently. It's possible we could unify them with a redesign of
    // what query plans look like, but it doesn't seem obvious how.
    //
    // Atoms get tries built for them; operators don't.
    //
    // Atoms get consulted on every variable they touch; operators, for now, only on the
    // last one (see "Limitations of this trait for operators" below).
    //
    // Because we separate atoms from operators, we only pay dispatch overhead for
    // operators; queries without operators don't pay.
}

pub struct Atom<Rel, Var> {
    pub relation: Rel,
    pub vars: Vec<Var>,         // Invariant: vars.len() == db.arity(relation)
    // TODO: how do we represent constants in atoms?
}

#[derive(Clone)]
pub struct OpCall<Op, Var> {
    pub op: Op,
    pub vars: Vec<Var>,
    // Invariant: vars.len() == op.arity(). The first op.input_arity() vars are inputs. If
    // op.has_output(), the last var is the output. In a Query the `vars` are query Vars;
    // in a QueryPlan they are indexes into the variable order (OpCall<Op, usize>).
}

impl<Var: Eq+Hash+Copy, Rel: Eq+Hash+Clone, Op: Operator> Query<Var, Rel, Op> {
    #[allow(unreachable_code, dead_code, unused)]
    fn self_check<Db: Database<Rel = Rel>>(&self, db: &Db) {
        todo!("check: self.vars is distinct; no duplicates");
        todo!("check: every atom's length equals its relation's arity");
        todo!("check: every operator's vars.len() == arity");
        todo!("check: reject zero-variable operators (input_arity == 0 && !has_output), because we don't know how to plan them yet; add TODO comment that we ought to support them");
        // A query is grounded if all its vars are grounded. An atom grounds all its
        // variables. An operator grounds its output if its inputs are grounded. By
        // applying these rules to saturation we can find the grounded variables.
        todo!("check: query is grounded.");
    }
}


// ---------- OPERATORS, or ATOMS THAT COMPUTE ----------

// We parameterize Query over an Operator type because it lets us choose how to dispatch
// on operators. If we have a concrete `enum MyOps { ... }` for all the query operators we
// need, we can implement `Operator MyOps` by matching on this enum for performance
// (especially on WebAssembly where indirect calls through function pointers may be extra
// slow, according to Claude). But we can also use `Box<dyn Operator>` to mix-and-match
// Operator impls without writing a big enum & match.
//
// TODO: provide some examples that show how to do each of these.

pub trait Operator {
    fn input_arity(&self) -> usize;
    fn has_output(&self) -> bool; // at most one output, for now.
    fn arity(&self) -> usize { self.input_arity() + (self.has_output() as usize) }
    // Precondition: args.len() == self.arity().
    fn check(&self, args: &[Value]) -> bool;
    // Precondition: self.has_output() && inputs.len() == self.input_arity().
    fn compute(&self, inputs: &[Value]) -> Option<Value>;
}

#[allow(dead_code)]
#[derive(Clone)]
enum Empty {}                   // useful representation if your query has no operators.
impl Operator for Empty {
    #[inline] fn input_arity(&self) -> usize { match *self {} }
    #[inline] fn has_output(&self) -> bool { match *self {} }
    #[inline] fn check(&self, _: &[Value]) -> bool { match *self {} }
    #[inline] fn compute(&self, _: &[Value]) -> Option<Value> { match *self {} }
}

// This impl lets us use Rc<dyn Operator> (the default) or Box<dyn Operator> as our Operator
// representation in Queries. The plan clones operator handles (see C3 / QueryPlan), so the
// default is Rc, which is cheap to clone.
impl<Ptr> Operator for Ptr where
    Ptr: std::ops::Deref<Target = dyn Operator>,
{
    fn input_arity(&self) -> usize { (**self).input_arity() }
    fn has_output(&self) -> bool { (**self).has_output() }
    fn arity(&self) -> usize { (**self).arity() }
    fn check(&self, args: &[Value]) -> bool {
        debug_assert!(args.len() == (**self).arity());
        (**self).check(args)
    }
    fn compute(&self, inputs: &[Value]) -> Option<Value> {
        debug_assert!(inputs.len() == (**self).input_arity());
        (**self).compute(inputs)
    }
}

// Limitations of our operator / OpCall representation:
//
// 0. Fixed input/output. Eg. if OpEq is an Operator for equality, then (x = y) can become:
//
//      OpCall { op: OpEq, vars: [x,y] }        which makes x input and y output
//      OpCall { op: OpEq, vars: [y,x] }        which makes y input and x output
//
//    But we can't have one OpCall that represents both and leaves it up to the planner to
//    decide which way information flows.
//
// 1. An operator must have 0 or 1 output variables.
//
// 2. Inputs must functionally determine the output.
//
// To drop these restrictions we must redesign this interface and modify the variable
// order picker and perhaps also the representation of query plans.
//
// The var order picker will exploit (2) by emitting output vars immediately when their
// inputs become available. This is desirable only when the outputs are uniquely
// determined by the inputs!
//
// The query plan/executor exploits (1) by only including/consulting a operator on the
// level for its last variable. (If that final variable is its output, the atom can
// propose values; if it is one of its inputs, it can check consistency.) But if we had
// multiple output variables, OR if the output var were not guaranteed to be examined
// immediately after the inputs (if we add operators whose outputs are not unique), then
// we should consult the atom as soon as all input are bound, and again whenever we pick
// an output var. This considerably complicates the interaction between operators and the
// rest of the query.
//
// Here are some examples of operators we might want, and their properties.
//
//              SYNTAX          INPUTS      OUTPUTS     FD?
// INEQUALITY   x ≤ y           x,y         none        trivial
//
// CONSTANT     x = 2           none        x           yes
//
// EQUALITY     x = y           x           y           yes, ∀x ∃!x  x=y
//                              y           x           yes, ∀y ∃!x  x=y
//
// RANGE        i ∈ range(n,m)  n,m         i           no
//
// ADDITION     x = y + z       y,z         x           yes; ∀y,z ∃!x  x = y + z
//                              x,z         y           yes; at most one y for fixed (x,z)
//                              x,y         z           yes; at most one z for fixed (x,y)
//
// STRING       x = y ++ z      y,z         x           yes; ∀y,z ∃!x
// APPEND                       x           y,z         no; many y, z yield same x = y ++ z
//                              x,y         z           yes; ∀x,y ∃ at *most* one z
//                              x,z         y           yes; ∀x,z ∃ at most one y
//
// Addition & string append are good examples of operators with multiple possible
// input-output modes. Given strings y,z we can compute x = y ++ z. But given x, we can
// ask for all y ++ z = x, all "splittings" of it. And given x,y we can ask: is y a prefix
// of x, and if so, what's the suffix?
//
// With our current approach the input/output mode is hard-coded into the OpCall and thus
// the query. This is probably okay for many queries, but it means the variable order
// picker has less room to choose how to order computation.

// ==== ON IMPLEMENTING OPERATORS EFFICIENTLY ====
//
// It's possible dispatch overhead for operators will become a bottleneck for query
// execution in some cases. If so, it's worth investigating Frank McSherry's approach to
// them, which attempts to dispatch as infrequently as possible by having operators
// process large "chunks" of data at a time.
//
// He does this via a breadth-first approach to solving WCOJs, maintaining a vector of
// partial solutions for the first N variables, then extending to partial solutions for
// N+1, etc. To see an example of this, look at the examples/join-v1.rs prototype. See
// also his blog post:
//
// https://github.com/frankmcsherry/blog/blob/master/posts/2025-12-23.md
//
// Email me (Michael Arntzenius, daekharel@gmail.com) if you're having trouble
// understanding it or how it relates to this implementation; or email Frank and cc me,
// he's quite friendly (but won't know anything about this implementation).
//
// Note that it's a little more complicated than just switching depth for breadth. The
// Claude-generated bfs in join_bfs.rs, for instance, is not Frank-like and would not
// improve performance. Also, breadth-first search can be very memory hungry if there are
// lots of results; if this is a problem it might be worth doing something in-between BFS
// & DFS involving fixed-length chunks of partial solutions.
//
// Our current approach is depth-first and more like a hash-based version of LFTJ. You can
// read the LFTJ paper if you like, but this code is probably easier to understand.
//
// LFTJ paper: https://arxiv.org/abs/1210.0481
//
// LFTJ uses a "trie iterator" interface. If you line things up right it's possible for
// many computational atoms to satisfy this interface. You can think of this as
// materializing the trie lazily/on-demand. Of course, computational atoms can't
// materialize levels of the trie that correspond to their *input* variables, but they
// can be told to "seek to position x" (this assigns that input variable to x). As long
// as *some* atom/trie iterator can materialize a list of candidates for this variable,
// things work out eventually.
//
// See section 3.4, p6, list item 1, which discusses equality atoms, and section 6.2,
// numbered list, elements 2-3 ("Functions", "Primitives") and 6 ("Ranges"). (Note that
// "Function" does NOT mean computational function here: it means functional dependency.)


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


// ---------- QUERY PLANNING ----------
pub struct QueryPlan<Rel, Op> {
    // Replaces query atoms by the shape of the index we need for them.
    pub atoms: Vec<(Rel, IndexShape)>,
    // One level per variable, in variable order. levels[i] describes what to do when we
    // bind variable i: which atoms constrain it, and which operators to consult.
    pub levels: Vec<Level<Op>>,
}

// Everything the executor needs to do at a single variable's level.
#[derive(Clone)]
pub struct Level<Op> {          // TODO: rename to VarPlan?
    // An operator to propose a unique value for this variable, computed from its inputs
    // (its output is this variable). If None, we propose from the trie node with the
    // fewest children.
    //
    // Once we have FDs and may know statically that a relation will propose exactly one
    // value, this can become `enum Proposer { Atom(usize), Op(OpCall<Op, usize>) }`.
    pub proposer: Option<OpCall<Op, usize>>,
    // Operators that filter bindings.
    pub filters: Vec<OpCall<Op, usize>>,
    // The indexes in QueryPlan.atoms of the atoms constraining this variable.
    pub atoms: Vec<usize>,
}

// For example, Query { vars: [x,y,z], atoms: [R(x,y), R(y,z)] }.plan() yields:
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
//              Level { atoms: [], proposer: Some(OpCall { vars: [0,2,3] }), filters: [] }
//
// TODO: some unit tests for Query::plan.
// Try this, and the same with var order [y,x,z].
// Try some with multiple uses of the same variable, or (once implemented) constants.

// ========== TODO: review & cleanup this LLM code: ==========
impl<Var, Rel, Op> Query<Var, Rel, Op> where
    Var: Eq + Hash + Copy,
    Rel: Eq + Hash + Clone,
    Op: Operator + Clone,
{
    pub fn plan(&self, order: &[Var]) -> QueryPlan<Rel, Op> {
        use IndexColumnShape::{EqColumn, TrieLevel};

        // TODO: check that order is a permutation of the query variables.

        // order_pos[v] = position of variable v in the global variable order.
        let order_pos: HashMap<Var, usize> =
            order.iter().enumerate().map(|(i, &v)| (v, i)).collect();
        assert_eq!(order_pos.len(), order.len(), "variable order repeats a variable");

        let mut atoms: Vec<(Rel, IndexShape)> = Vec::with_capacity(self.atoms.len());
        let mut levels: Vec<Level<Op>> = (0..order.len())
            .map(|_| Level { atoms: Vec::new(), proposer: None, filters: Vec::new() })
            .collect();

        for (a, atom) in self.atoms.iter().enumerate() {
            // first_col[v] = the first column of this atom where variable v appears
            // (or_insert keeps the earliest column). `distinct` is its key set; its order
            // doesn't matter — we sort it for levels and otherwise treat it as a set.
            let mut first_col: HashMap<Var, usize> = HashMap::default();
            for (col, &v) in atom.vars.iter().enumerate() {
                first_col.entry(v).or_insert(col);
            }
            let distinct: Vec<Var> = first_col.keys().copied().collect();

            // Assign each distinct variable a trie level: its rank among this atom's
            // variables when sorted by global order position. This is what keeps the atom's
            // trie aligned with the global binding order.
            let mut by_order = distinct.clone();
            by_order.sort_by_key(|v| {
                *order_pos.get(v).expect("every atom variable must appear in the variable order")
            });
            let level_of: HashMap<Var, usize> =
                by_order.iter().enumerate().map(|(lvl, &v)| (v, lvl)).collect();

            // Build the shape column by column: a variable's first occurrence becomes a
            // TrieLevel; a repeat becomes an EqColumn back-reference to its first column.
            // (A constant would become EqConst here, once Atom can carry constants.)
            let mut shape: IndexShape = Vec::with_capacity(atom.vars.len());
            for (col, &v) in atom.vars.iter().enumerate() {
                if first_col[&v] == col {
                    shape.push(TrieLevel(level_of[&v]));
                } else {
                    shape.push(EqColumn(first_col[&v]));
                }
            }

            // This atom binds each of its distinct variables' order positions.
            for &v in &distinct { levels[order_pos[&v]].atoms.push(a); }
            atoms.push((atom.relation.clone(), shape));
        }

        // Each operator is consulted at the level of its *last* variable in `order` (the one
        // with the greatest order position): by then every other var it touches is bound.
        for opcall in &self.operators {
            let op = &opcall.op;
            let n_in = op.input_arity();
            assert_eq!(opcall.vars.len(), n_in + op.has_output() as usize,
                "operator vars don't match its arity (inputs first, then the output if any)");
            assert!(!opcall.vars.is_empty(), "zero-variable operators are not supported");

            // Order positions of the operator's variables (inputs first, then output).
            let positions: Vec<usize> = opcall.vars.iter().map(|v| {
                *order_pos.get(v).expect("every operator variable must appear in the variable order")
            }).collect();
            let last_pos = *positions.iter().max().unwrap();

            let output_pos: Option<usize> = op.has_output().then(|| positions[n_in]);

            // Propose if the output is this level's variable (the last one to be bound) and
            // the proposer slot is still free; otherwise this operator is a filter.
            let level = &mut levels[last_pos];
            let step = OpCall { op: op.clone(), vars: positions };
            if output_pos == Some(last_pos) && level.proposer.is_none() {
                level.proposer = Some(step);
            } else {
                level.filters.push(step);
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

// ========== END LLM CODE ==========

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
    children: Vec<&'a Trie>,    // trie children for descending to next level
    input_buf: Vec<Value>,      // values for input to an Operator
}

impl<'a, Op: Operator> ExecutableQuery<'a, Op> {
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
            input_buf: Vec::new(),
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

// ========== TODO: do a review/simplification pass over this LLM-modified code ==========
impl<'a, Op: Operator, F: FnMut(&[Value])> QueryDfsState<'a, Op, F> {
    // Copy the given prefix positions into `input_buf`, in order.
    fn gather(&mut self, positions: &[usize]) {
        self.input_buf.clear();
        for &pos in positions { self.input_buf.push(self.prefix[pos]); }
    }

    // Run this level's filter operators.
    fn filters_pass(&mut self, filters: &[OpCall<Op, usize>]) -> bool {
        for f in filters {
            self.gather(&f.vars);
            if !f.op.check(&self.input_buf) { return false; }
        }
        true
    }

    fn execute(&mut self, level_idx: usize) {
        // Copy the `&'a` levels slice out so `level` borrows the plan, not `self`, and can be
        // held across the `&mut self` calls below.
        let levels = self.levels;
        if level_idx == levels.len() {
            (self.callback)(&self.prefix);
            return;
        }
        let level: &'a Level<Op> = &levels[level_idx];
        // Snapshot the current node of each trie in this level onto the `saved` stack so we
        // can restore them when we're done; `mark` is where this level's slice begins.
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
                let key = match prop.op.compute(&self.input_buf) {
                    Some(v) => v,
                    None => { self.restore(level, mark); return; } // proposal failed.
                };
                // Look up the proposed key in each level trie; a miss kills this branch.
                self.children.clear();
                for pos in 0..width {
                    match self.saved[mark + pos].get(&key) {
                        Some(child) => self.children.push(child),
                        None => { self.restore(level, mark); return; }
                    }
                }
                self.descend(level, level_idx, key);
            }
            // No operator proposer: the trie with the fewest children proposes, and we
            // intersect the proposed key against the other level tries. We read each level
            // trie's map off the `saved` stack (positions mark..mark+width).
            None => {
                let proposer_pos: usize = (0..width)
                    .min_by_key(|&pos| self.saved[mark + pos].len())
                    .expect("no proposer at this level - the planner should have caught this");
                let proposer_map = self.saved[mark + proposer_pos];
                'keys: for (key, child) in proposer_map {
                    self.children.clear();
                    // Look up this key in each trie at this level. If any trie lacks this
                    // key, skip to the next key.
                    for pos in 0..width {
                        if pos == proposer_pos { self.children.push(child); continue; }
                        match self.saved[mark + pos].get(key) {
                            Some(child) => self.children.push(child),
                            None => continue 'keys,
                        }
                    }
                    self.descend(level, level_idx, *key);
                }
            }
        }

        self.restore(level, mark);
    }

    // Commit `key` as this level's value: descend each level trie to its child under `key`,
    // run the level's filters, and recurse if they all pass. Assumes `self.children` holds
    // the child of each level atom (in `level.atoms` order).
    fn descend(&mut self, level: &'a Level<Op>, level_idx: usize, key: Value) {
        self.prefix.push(key);
        for (pos, &trie_idx) in level.atoms.iter().enumerate() {
            // A Leaf child bottoms out here and is never read again, so we skip it.
            if let Trie::Node(map) = self.children[pos] { self.tries[trie_idx] = map; }
        }
        if self.filters_pass(&level.filters) {
            self.execute(level_idx + 1);
        }
        let popped = self.prefix.pop();
        debug_assert!(popped == Some(key));
    }

    // Restore every trie in this level to the parent node the caller left it at, then pop
    // this level's slice off the `saved` stack.
    fn restore(&mut self, level: &'a Level<Op>, mark: usize) {
        for (pos, &trie_idx) in level.atoms.iter().enumerate() {
            self.tries[trie_idx] = self.saved[mark + pos];
        }
        self.saved.truncate(mark);
    }
}
// ========== END code need review/simplifying ==========


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
