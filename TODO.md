# NOTES

Main file being developed right now is `examples/join-v2.rs`.

To run tests,

    cargo test --example join-v2

To run benchmark (ie `main` from join-v2.rs):

    cargo run --release --example join-v2

Use `--release` or else it will be slow.

# TODOs

- implement 4-clique (K4) benchmark, should show a more significant speedup than
  triangles compared with non-WCO join. Claude suggests comparing against a
  2-step binary join plan: find triangles, then extend to 4-cliques.

- implement breadth-first version of execute_dfs().
  check performance diff.

- maybe: debug perf of execute_dfs() using callgrind?
  would need to run it on Sully's AMD box.

- compare performance of undirected triangle search to dijkstralog.

- query planning: derive which trie indexes to build from a query + variable
  order and bundle them into a struct. Right now callers hand-build each index
  and QueryPlan by hand.

- FDs in the schema: figure out how to represent functional dependency info in
  the Database trait (e.g. per-relation primary keys), for FD chasing during
  planning.

- constants in atoms: decide how to represent constant arguments in Atom (e.g.
  R(x,2)). Trie::build already supports EqConst shapes; this is about the
  query-level representation.

- Trie::build perf: the row loop reinterprets filters and level_to_col per row.
  Measure the interpretive overhead; if it's significant, split into per-filter
  loops / a final level_to_col loop, or pipeline further.
