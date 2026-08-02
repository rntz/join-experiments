# NOTES

Main file being developed right now is `examples/join-v2.rs`.

To run tests,

    cargo test --example join-v2

To run benchmark (ie `main` from join-v2.rs):

    cargo run --release --example join-v2

Use `--release` or else it will be slow.

# TODOs

- why is snap_triangles_directed("twitter_combined.txt", None) failing?
  why are we getting different #s of triangles from the binary vs WCO joins?
  could try writing triangles to files and diffing them?
  but there are so many!
  HUGE difference in # of triangles, over 100 billion!

      Reading all edges...
      2420766 edges, sorting... done!
      twitter_combined.txt: 2420766 edges -> 20811839 directed triangles
        wcoj build    224.308417ms
        wcoj execute  3.066438084s
        wcoj total    3.290752167s    found 20811839 triangles
        2-edge-filter 14.555838542s    found 152435135 triangles

- move FxHash up to the top of the file, separate from other things. name types
  so that it's a one-line change to switch from default (SipHash) to FxHash.

- implement 4-clique (K4) benchmark, should show a more significant speedup than
  triangles compared with non-WCO join. Claude suggests comparing against a
  2-step binary join plan: find triangles, then extend to 4-cliques.

- implement breadth-first version of execute_dfs().
  check performance diff.

- maybe: debug perf of execute_dfs() using callgrind?
  would need to run it on Sully's AMD box.

- compare performance of undirected triangle search to dijkstralog!
