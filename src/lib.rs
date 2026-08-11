// Worst-case-optimal join experiments. The core engine lives in `join`; `hash` holds the
// fast non-cryptographic hasher everything is parameterized over; `vec_db` and `graph` are
// utilities shared by the benchmark example and the integration tests.

// Print sans trailing newline and flush immediately, so progress output shows up before
// the following (possibly slow) work. Exported so `graph` and the benchmark can use it
// without pulling in `std::io::Write`.
#[macro_export]
macro_rules! print_flush {
    ($($e:tt)*) => {{
        print!($($e)*);
        std::io::Write::flush(&mut std::io::stdout()).unwrap()
    }};
}

#[macro_export]
macro_rules! row {
    [$($e:expr),*] => (vec![$($crate::value::ValueType::to_value($e)),*])
}

pub mod value;                // value representation
pub mod hash;                 // FxHash and Map/Set based on hash
pub mod op;                   // computational operators
pub mod join;                 // databases, queries, query plans & execution
pub mod var_order;            // variable order planning
pub mod join_bfs;             // breadth-first query execution prototype
// these are mainly for tests & benchmarking:
pub mod vec_db;               // in-memory vec-of-row-vectors database
pub mod graph;                // graph db utilities

pub use value::{Value, ValueType, TagError};
pub use op::Operator;
pub use join::{
    Atom, Database, ExecutableQuery, IndexColumnShape, Indexes, Level,
    QueryPlan, Query, Trie,
};
pub use vec_db::VecDb;
pub use graph::{
    binary_triangles_directed, binary_triangles_undirected, edge_db, snap_load, symmetrize,
    to_low_high,
};
