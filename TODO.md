# NOTES

Build:

    cargo build --all-targets

Tests:

    cargo test

Benchmarks (remember use use `--release`):

    cargo run --release --example triangles

You'll need to download some SNAP datasets first:

    ./download_snap_datasets.sh

This can be slow depending on your network connection.

# High level goals not started on yet

- computational atoms: computing functions on attribute values
- aggregations
- mutation & incremental maintenance over it

# Pieces of joins I haven't implemented yet

- interning!
- constants in queries!
- variable orders!
- deriving indexes & query plan given a variable order!

# Nice to haves I haven't implemented yet

- chasing FDs
- semijoins

# TODOs

- fix bug: Consider `E(x,y) T(5)`. The atom `T(5)` will build a depth-0 `Trie::Leaf` index
  and hit the "unreachable!()" case in execute_dfs. Probably also true of _bfs.

- Problem: empty index -> can't even build a QueryPlan because we need Trie not
  Option<Trie>.

- query planning: derive which trie indexes to build from a query + variable
  order and bundle them into a struct. Right now callers hand-build each index
  and QueryPlan by hand.

- FDs in the schema: figure out how to represent functional dependency info in
  the Database trait (e.g. per-relation primary keys), for FD chasing during
  planning.

- constants in atoms: decide how to represent constant arguments in Atom (e.g.
  R(x,2)). Trie::build already supports EqConst shapes; this is about the
  query-level representation.

# TODOs for later

- implement 4-clique (K4) benchmark, should show a more significant speedup than
  triangles compared with non-WCO join. Claude suggests comparing against a
  2-step binary join plan: find triangles, then extend to 4-cliques.

- maybe: debug perf of execute_dfs() using callgrind?
  would need to run it on Sully's AMD box.

- compare performance of undirected triangle search to dijkstralog.
