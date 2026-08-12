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
// Get an ExecutableQuery by binding indexes to plan.
let Some(exec) = plan.bind(&indexes) else {
  // If a relation involved is empty, binding the plan will fail;
  // this indicates no query results.
  return;
}
// Iterate over solutions.
exec.execute_dfs(|solution| {
    // solution[i] = value for of var_order[i].
    // If you want them in the order of `query.vars`, do the remapping here.
    do_something_with(solution)
})
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

## Chasing FDs

TODO
