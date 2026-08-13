# rntz-joins

Experiments in worst-case-optimal join (WCOJ) evaluation. Relations are indexed as
hash-based tries; a query plan intersects them one variable at a time, either depth-first
(backtracking) or breadth-first (frontier of partial solutions).

## Source map

| File | Contents |
|------|----------|
| `src/lib.rs` | Crate root: module wiring and re-exports. |
| `src/value.rs` | The `Value` type (interned as `usize`) that everything joins over. |
| `src/join.rs` | Core engine: databases, queries, tries, query plans, query execution. Many design comments. |
| `src/var_order.rs` | Variable order picker. |
| `src/op.rs` | Computational operators trait & implementations (eg. addition, ≤). |
| `src/join_bfs.rs` | Breadth-first query execution prototype. Feel free to delete this. |
| `src/hash.rs` | `FxHasher` (fast non-cryptographic hash). Edit the `HashBuilder` definition in this file to switch from FxHash to Rust's default SipHash. |
| `src/vec_db.rs` | Trivial vector-based `Database` used by tests & benchmarks. |
| `src/graph.rs` | SNAP dataset loading and reference triangle finders using binary joins. |
| `tests/queries.rs` | Simple query correctness tests on tiny data. |
| `tests/self_check.rs` | Tests for `Query::{ground_vars, self_check}` (query well-formedness). |
| `tests/var_order.rs` | Tests for `Query::structural_var_order`. |
| `examples/triangles.rs` | Triangle-counting benchmark over SNAP graphs. |
| `examples/join-v1.rs` | Earlier standalone prototype; superseded by the library. |
| `download_snap_datasets.sh` | Fetches SNAP graph datasets into `data/`. |

## Usage

```sh
cargo build --all-targets                   # does everything build?
cargo test                                  # run test suite
./download_snap_datasets.sh                 # download data needed for benchmarks into data/
cargo run --release --example triangles     # run benchmarks
```

## Overview: how does query execution work?

Read `src/join.rs`. Here's the pipeline for executing a query currently:

1. Make a query.
2. Pick a variable order on it.
3. Build indexes on your database using the variable order.
4. Execute the query against those indexes.

In code (see `examples/basic.rs`):

```rust
let db = ...; // make a database whose type satisfies `Database`
let query = Query { ... };
query.self_check(&db); // check query is well-formed.
let var_order = query.structural_var_order();
let plan = query.plan(&var_order);
let indexes = plan.build_indexes(&db);
// Get an ExecutableQuery by binding indexes to plan. If a relation involved is
// empty, binding the plan will fail; this indicates no query results.
if let Some(exec) = plan.bind(&indexes) {
    // Iterate over solutions.
    exec.execute_dfs(|solution| {
        // solution[i] = value for of var_order[i].
        // If you want them in the order of `query.vars`, do the remapping here.
        do_something_with(solution)
    });
}
```

### How *should* query execution work?

There are some steps we might like to add here:

TODO DESCRIBE

## Things to do next

- Support constants in atoms. The trie indexing machinery for this is already present. The
  operator machinery in ExecutableQuery::execute_dfs will need to be adjusted a little.

- Split Schema out of the Database trait. Schema can probably just be a struct that maps
  relation ids to info about them (arity, FDs).

- Support FD information. The simplest approach: each relation in a Schema gets an
  optional primary key. This matches well with ACSets, where every entity type would get a
  single relation with a primary key.

- Use FD information in the variable order picker: pick determined variables immediately.
  We already do this for operators.

- Implement mutation & incremental maintenance. See [Mutation](#mutation).

## Optional, bigger todos

- Switch from a tagged to an interned representation for values, or a smarter tagging
  regime & execution engine to reduce tag-checking overhead. See `src/value.rs`. I've
  implemented both tagged & interned value representations but not implemented
  interning/deinterning, and interning is somewhat complicated if you want to de-allocate
  interned values correctly over database updates. Doing tagging in a smarter way might be
  the best approach but would require significant rewriting - not just to use fewer tags,
  but to check them less often in query execution.

- Chase FDs in the query. This has the potential to significantly improve performance.
  See [Chasing FDs](#chasing-fds).

- Implement semijoin reduction and GYO. This can improve performance asymptotically on
  some queries. It might be best to do this only if you actually see a performance issue;
  and, before you do it, validate that the problem would be solved by doing semijoin
  reduction with whiteboard math or prototyping. An even more general asymptotic
  improvement is to do generalized hypertree decomposition of the query. I didn't have
  time to investigate this and write it up, unfortunately.

## Mutation

This is probably the most important thing I didn't get to.

As I see it, mutation is not really a data structures problem; it's an incremental view
maintenance problem. The query engine doesn't care how the Database stores data; the first
thing it does is build its own indexes, and query execution exclusively uses those. We
only need a way to read the contents of the database (`Database::scan`).

Similarly, to handle mutation, we need to know what changed (the diff). The simplest way
to do this is to say, for every relation in the query, what got added and what got
removed. Something like:

```rust
trait DatabaseDiff {
    type Rel: Eq + Hash + Clone;
    // Invariant: insertions/deletions are disjoint. We may also wish to insist they are
    // minimal, i.e. we don't insert existing rows or delete non-existent rows.
    fn scan_inserts<F: FnMut(&[Value])>(&self, r: Self::Rel, callback: F);
    fn scan_deletes<F: FnMut(&[Value])>(&self, r: Self::Rel, callback: F);
}
```

We then wish to translate a `DatabaseDiff` into a diff to the results of the query. To do this, we need a few ingredients:

1. We need to derive & perform delta queries. A delta query computes the diff to a query's results given the diff to the database. There is a standard literature on these.
2. These delta queries will need indexes. These indexes may not be the same as those needed by the original query execution step.
3. Therefore, we should derive and plan these delta queries when we plan the original query, so that we know the indexes they need and can build them for the original, unmodified database.
4. We must update these indexes using the database diff so we can use them the next time the database changes.

The indexes act as intermediate state passed between the original query execution and the
incremental maintenance passes. You can think of the whole system as a (not necessarily
finite) state machine, specifically a deterministic transducer, like a Moore or Mealy
machine, or an optic but for charts instead of lenses:

    initial pass: Input          → State × Output
    update pass:  State × ΔInput → State × ΔOutput

Here, Input = Database; ΔInput = DatabaseDiff; Output = query results; ΔOutput = diff to query results; and State = Indexes. As querying gets more elaborate (e.g. if you implement the ideas about chasing FDs or semijoin reductions), it may turn out that you need more state than just indexes; but the state-machine pattern will remain.

So, how do we derive these delta queries?

### Delta queries

The standard modern approach to IVM is to assume a ring or group structure -- this is the approach taken by [DBSP][] and its predecessor Differential Dataflow. This assumption is inconvenient for us. An older and simpler alternative is to maintain explicitly against disjoint sets of additions/removals. This has a few disadvantages, but they mostly apply for more complex queries (disjunctive and especially recursive queries).

[DBSP]: https://arxiv.org/abs/2203.16684
[fic]: https://arxiv.org/abs/1811.06069

The best presentation of this approach I've seen is [Fixing Incremental Computation][fic]. This paper purports to be about fixed points, but (a) we do not need fixed points, which is good because (b) I think it is wrong about them; [see this note](/note-on-fixing-incremental-computation.md). Instead, read the paper for the definition of change actions and for section 4 “Derivatives for non-recursive Datalog”, especially fig. 2 (p12), which shows delta query derivation for increasing/decreasing changes.

The core idea here is that the change to a conjunctive query is recoverable as a disjunction of conjunctions.

TODO EXPLAIN

### Factorizing delta queries

Consider the query

    Q(x,y,z) = E(x,y), E(y,z), E(x,z), x + y = z

Of these, only the `E` relation is capable of changing; the `+` relation is fixed. If we
assume only increasing changes, let `E` be the set of old tuples and `ΔE` be the set of
added tuples, the new tuples in `Q` are:

    ΔQ(x,y,z) =    (ΔE(x,y), ΔE(y,z), ΔE(x,z), x + y = z)
                or (ΔE(x,y), ΔE(y,z),  E(x,z), x + y = z)
                or (ΔE(x,y),  E(y,z), ΔE(x,z), x + y = z)
                or (ΔE(x,y),  E(y,z),  E(x,z), x + y = z)
                or ( E(x,y), ΔE(y,z), ΔE(x,z), x + y = z)
                or ( E(x,y), ΔE(y,z),  E(x,z), x + y = z)
                or ( E(x,y),  E(y,z), ΔE(x,z), x + y = z)

As you can see, we get 2³-1 combinations of ΔE and E involving at least one ΔE. However, 2ⁿ-1 is rather large; there's a much less combinatorially explosive way of factoring this. Let `τE = E ∪ ΔE`. Then:

    ΔQ(x,y,z) =    (ΔE(x,y),  E(y,z),  E(x,z), x + y = z)
                or (τE(x,y), ΔE(y,z),  E(x,z), x + y = z)
                or (τE(x,y), τE(y,z), ΔE(x,z), x + y = z)

Now we get only n cases, for n = the number of atoms that can change. Much better!
However, it requires a redundancy in our data representation: we simultaneously need
appropriate indexes on the old data `E` and the updated `τE`. This seems to require copying
the indexes for `E` instead of modifying them in place. This copying takes time
proportional to the size of `E`. This is problematic: we want delta maintenance to do work
proportional to the size of delta when possible; it is unacceptable for it to always take
time linear in the size of the database! There are a few options here:

1. Accept the more combinatorially explosive full-delta enumeration. I worry about this:
   10 atoms yields ~1,000 cases; 20 atoms, ~1 million cases; 30 atoms, ~1 billion. 1
   billion is definitely too many. If you do this long-term, make sure you plumb the error
   through to the user in a visible way.

2. Use an index data structure that can be updated [persistently][pds]. For instance,
   replace the hash tables in the trie nodes with balanced trees that support O(log n)
   insertion/removal while preserving the existing version. Because this results in
   structural sharing between the old & new tries, you'd need to use ref-counted `Rc<>`
   pointers to refer to subtries. In fact, the poor man's version of this is to *just* put
   `Rc<>` pointers on subtries and trie to preserve subtries that aren't modified, but
   copy hashtables willy-nilly. This might be good enough for a prototype.

3. Allow (disjoint) unions in query plans and make `execute_dfs` smarter. I think this is
   quite doable. Instead of each level being a vector of intersected atoms, it'd be an
   intersection-vector of union-vectors of atoms. Then `τE(x,z)` gets represented directly
   as `E(x,z) ∪ ΔE(x,z)`.

   Fundamentally, the operations we need are count(), propose(), and filter(). To count a
   union, sum the counts of its members (the count is actually representing work, not
   element count, so even if they're not disjoint, this is correct). To propose, propose
   everything from both (here you want to dedup if they're not disjoint). To filter, check
   if either accepts. Managing the trie state gets tricky but not impossible.

I think (2) or (3) are ideal but (1) might be good enough to get started and the poor
man's version of (2) is very doable and might be fast enough.

[pds]: https://en.wikipedia.org/wiki/Persistent_data_structure


## Chasing FDs

TODO
