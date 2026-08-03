# rntz-joins

Experiments in worst-case-optimal join (WCOJ) evaluation. Relations are indexed as
hash-based tries; a query plan intersects them one variable at a time, either depth-first
(backtracking) or breadth-first (frontier of partial solutions).

## Source map

| File | Contents |
|------|----------|
| `src/lib.rs` | Crate root: module wiring, re-exports, simple utilities. |
| `src/join.rs` | Core engine: databases, queries, tries, query plans, query execution. Many design comments. |
| `src/hash.rs` | `FxHasher` (fast non-cryptographic hash) and the `Map`/`Set` aliases used throughout. |
| `src/vec_db.rs` | `VecDb`, a trivial in-memory `Database`. |
| `src/graph.rs` | SNAP dataset loading and reference triangle finders using binary joins. |
| `tests/queries.rs` | End-to-end query tests (triangle, path, self-loop, undirected, bfs==dfs). |
| `examples/triangles.rs` | Triangle-counting benchmark over SNAP graphs. |
| `examples/join-v1.rs` | Earlier standalone prototype; superseded by the library. |
| `download_snap_datasets.sh` | Fetches SNAP graph datasets into `data/`. |

## Usage

```sh
cargo test                                  # run the test suite
cargo run --release --example triangles     # run the benchmark (needs data/, use --release)
```
