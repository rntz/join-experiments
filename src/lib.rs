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

pub mod graph;
pub mod hash;
pub mod join;
pub mod join_bfs;
pub mod vec_db;

pub use join::{
    Atom, Database, ExecutableQuery, IndexColumnShape, Indexes, PlannedQuery, Query, Trie, Value,
};
pub use vec_db::VecDb;
pub use graph::{
    binary_triangles_directed, binary_triangles_undirected, edge_db, snap_load, to_low_high,
};
