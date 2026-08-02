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

- compare performance of undirected triangle search to dijkstralog!
