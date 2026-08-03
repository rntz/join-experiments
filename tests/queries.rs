// (Mostly Claude-generated with minimal review.)
//
// End-to-end query tests: build trie indexes + a QueryPlan by hand (as a planner
// eventually would), then check execute_dfs/bfs against a brute-force computation.
use rntz_joins::IndexColumnShape::{EqColumn, TrieLevel};
use rntz_joins::{
    binary_triangles_directed, binary_triangles_undirected, edge_db, to_low_high, QueryPlan, Trie,
    Value, VecDb,
};

// ---- triangle query E(x,y) E(y,z) E(z,x), order x,y,z. ----
//
// The canonical worst-case-optimal-join workload, checked against brute force.
#[test]
fn test_triangle_query() {
    let edges: Vec<(Value, Value)> = vec![
        (0, 1), (1, 2), (2, 0),   // a directed 3-cycle
        (0, 2), (2, 1), (1, 0),   // and its reverse
        (1, 3), (3, 1),           // extra edges, not in any triangle here
    ];
    let db = edge_db(&edges);
    // fwd = E indexed (source, dest); bwd = E indexed (dest, source). Rewritten
    // atoms: fwd(x,y) fwd(y,z) bwd(x,z).
    let fwd = Trie::build(&db, "E", &vec![TrieLevel(0), TrieLevel(1)]).unwrap();
    let bwd = Trie::build(&db, "E", &vec![TrieLevel(1), TrieLevel(0)]).unwrap();
    let plan = QueryPlan {
        tries: vec![&fwd, &fwd, &bwd],
        levels: vec![vec![0, 2], vec![0, 1], vec![1, 2]],
    };
    let got = plan.collect_dfs();
    let want = binary_triangles_directed(&edges);

    assert!(!want.is_empty(), "test data should contain triangles");
    assert_eq!(got, want, "triangle join mismatch");
}

// ---- two-atom path query E(x,y) E(y,z), order x,y,z. ----
//
// A trie shared by two atom-entries (both use `fwd`), so it exercises the save/restore of a
// trie that participates in multiple levels.
#[test]
fn test_path_query() {
    let edges: Vec<(Value, Value)> = vec![
        (0, 1), (1, 2), (1, 3), (2, 3), (3, 0),
    ];
    let db = edge_db(&edges);
    let fwd = Trie::build(&db, "E", &vec![TrieLevel(0), TrieLevel(1)]).unwrap();

    // levels: x <- entry0; y <- entry0 ∩ entry1; z <- entry1.
    let plan = QueryPlan {
        tries: vec![&fwd, &fwd],
        levels: vec![vec![0], vec![0, 1], vec![1]],
    };
    let got = plan.collect_dfs();

    let mut want: Vec<Vec<Value>> = Vec::new();
    for &(x, y) in &edges {
        for &(y2, z) in &edges {
            if y2 == y { want.push(vec![x, y, z]); }
        }
    }
    want.sort_unstable();

    assert!(!want.is_empty(), "test data should contain 2-paths");
    assert_eq!(got, want, "path join mismatch");
}

// ---- single self-join atom R(x,x), order x. ----
//
// Exercises the EqColumn trie inside execute_dfs (a depth-1 join whose only trie came from
// a variable-reuse shape).
#[test]
fn test_self_loop_query() {
    let db = VecDb::new().rel(
        "R", 2,
        vec![vec![0, 0], vec![1, 1], vec![2, 3], vec![4, 4], vec![5, 6]],
    );
    let diag = Trie::build(&db, "R", &vec![TrieLevel(0), EqColumn(0)]).unwrap();
    let plan = QueryPlan { tries: vec![&diag], levels: vec![vec![0]] };
    let got = plan.collect_dfs();
    assert_eq!(got, vec![vec![0], vec![1], vec![4]], "self-loop mismatch");
}

#[test]
fn test_undirected_triangle_query() {
    // Raw edges with mixed orientation, a self-loop, and a duplicate — all normalized away.
    let raw: Vec<(Value, Value)> = vec![
        (1, 0), (1, 2), (2, 0),   // triangle {0,1,2}
        (0, 3), (3, 4), (4, 0),   // triangle {0,3,4}
        (2, 2),                   // self-loop -> dropped
        (0, 1),                   // duplicate of (1,0) after reorientation
    ];
    let edges = to_low_high(&raw);
    assert_eq!(edges, vec![(0, 1), (0, 2), (0, 3), (0, 4), (1, 2), (3, 4)]);

    let db = edge_db(&edges);
    let fwd = Trie::build(&db, "E", &vec![TrieLevel(0), TrieLevel(1)]).unwrap();
    let plan = QueryPlan {
        tries: vec![&fwd, &fwd, &fwd],
        levels: vec![vec![0, 2], vec![0, 1], vec![1, 2]],
    };
    let got = plan.collect_dfs();
    let want = binary_triangles_undirected(&edges);
    assert_eq!(got, want, "undirected join vs brute force");
    assert_eq!(got, vec![vec![0, 1, 2], vec![0, 3, 4]], "expected exactly two triangles");
}

// ---- execute_bfs matches execute_dfs. ----
//
// The breadth-first executor should compute exactly the same result set as the depth-first
// one on every plan. Cross-check on the triangle query (shared + swapped tries), the path
// query (a trie shared across two levels), the self-loop query (a depth-1 EqColumn trie),
// and the empty-query edge case.
#[test]
fn test_bfs_matches_dfs() {
    // Directed-triangle setup.
    let edges: Vec<(Value, Value)> = vec![
        (0, 1), (1, 2), (2, 0), (0, 2), (2, 1), (1, 0), (1, 3), (3, 1),
    ];
    let db = edge_db(&edges);
    let fwd = Trie::build(&db, "E", &vec![TrieLevel(0), TrieLevel(1)]).unwrap();
    let bwd = Trie::build(&db, "E", &vec![TrieLevel(1), TrieLevel(0)]).unwrap();
    let tri = QueryPlan {
        tries: vec![&fwd, &fwd, &bwd],
        levels: vec![vec![0, 2], vec![0, 1], vec![1, 2]],
    };

    // Path query (a single trie shared across two levels).
    let pedges: Vec<(Value, Value)> = vec![(0, 1), (1, 2), (1, 3), (2, 3), (3, 0)];
    let pdb = edge_db(&pedges);
    let pfwd = Trie::build(&pdb, "E", &vec![TrieLevel(0), TrieLevel(1)]).unwrap();
    let path = QueryPlan { tries: vec![&pfwd, &pfwd], levels: vec![vec![0], vec![0, 1], vec![1]] };

    // Self-loop query (a depth-1 EqColumn trie).
    let sdb = VecDb::new().rel(
        "R", 2, vec![vec![0, 0], vec![1, 1], vec![2, 3], vec![4, 4], vec![5, 6]],
    );
    let diag = Trie::build(&sdb, "R", &vec![TrieLevel(0), EqColumn(0)]).unwrap();
    let loop_ = QueryPlan { tries: vec![&diag], levels: vec![vec![0]] };

    // Empty query (no variables): both should yield exactly one empty tuple.
    let empty = QueryPlan { tries: vec![], levels: vec![] };

    for plan in [&tri, &path, &loop_, &empty] {
        assert_eq!(plan.collect_dfs(), plan.collect_bfs(), "bfs and dfs disagree");
    }
}
