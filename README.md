# rntz-joins

Experiments in worst-case-optimal join (WCOJ) evaluation. Relations are indexed as
hash-based tries; a query plan intersects them one variable at a time, either depth-first
(backtracking) or breadth-first (frontier of partial solutions).

## Source map

| File | Contents |
|------|----------|
| `src/lib.rs` | Crate root: module wiring, re-exports, simple utilities. |
| `src/join.rs` | Core engine: databases, queries, tries, query plans, query execution. Many design comments. |
| `src/hash.rs` | `FxHasher` (fast non-cryptographic hash). Edit the `HashBuilder` definition in this file to switch from FxHash to Rust's default SipHash. |
| `src/vec_db.rs` | Trivial vector-based `Database` used by tests & benchmarks. |
| `src/graph.rs` | SNAP dataset loading and reference triangle finders using binary joins. |
| `tests/queries.rs` | Simple query correctness tests on tiny data. |
| `examples/triangles.rs` | Triangle-counting benchmark over SNAP graphs. |
| `examples/join-v1.rs` | Earlier standalone prototype; superseded by the library. |
| `download_snap_datasets.sh` | Fetches SNAP graph datasets into `data/`. |

## Usage

```sh
cargo test                                  # run the test suite
cargo run --release --example triangles     # run the benchmark (needs data/, use --release)
```
