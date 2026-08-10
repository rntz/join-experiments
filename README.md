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
| `src/join_bfs.rs` | Breadth-first query execution prototype. |
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
cargo test                                  # run the test suite
cargo run --release --example triangles     # run the benchmark (needs data/, use --release)
```
