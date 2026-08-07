// (Mostly Claude-generated with minimal review.)
//
// End-to-end query tests: build a Query, plan() it, materialize its trie indexes, bind()
// into an ExecutableQuery, and check execute_dfs/bfs against a binary-join equivalent. We
// use `char` variables (x, y, z) and &str relation names.
use rntz_joins::IndexColumnShape::{EqColumn, TrieLevel};
use rntz_joins::{
    binary_triangles_directed, binary_triangles_undirected, edge_db, to_low_high, Atom,
    Query, QueryPlan, Value, VecDb,
};

// An atom over relation `rel` with the given variables. Sugar to keep the queries terse.
fn atom(rel: &'static str, vars: &[char]) -> Atom<&'static str, char> {
    Atom { pred: rel, vars: vars.to_vec() }
}

// The `atoms` list of each plan level, in order. These operator-free plan tests check the
// atom indexes per level; a Level now also carries proposer/filter operators (none here).
fn level_atoms<R, Op>(plan: &QueryPlan<R, Op>) -> Vec<Vec<usize>> {
    plan.levels.iter().map(|l| l.atoms.clone()).collect()
}

// ---- planning the directed triangle: two shared indexes, the documented plan. ----
#[test]
fn test_plan_triangle() {
    let edges: Vec<(Value, Value)> = vec![
        (0, 1), (1, 2), (2, 0), (0, 2), (2, 1), (1, 0), (1, 3), (3, 1),
    ];
    let db = edge_db(&edges);
    let q: Query<char, &'static str> = Query {
        vars: vec!['x', 'y', 'z'],
        atoms: vec![atom("E", &['x', 'y']), atom("E", &['y', 'z']), atom("E", &['z', 'x'])],
        operators: vec![],
    };
    let plan = q.plan(&['x', 'y', 'z']);

    // fwd = [TrieLevel(0),TrieLevel(1)] for E(x,y) and E(y,z); bwd = swapped for E(z,x).
    assert_eq!(plan.atoms, vec![
        ("E", vec![TrieLevel(0), TrieLevel(1)]),
        ("E", vec![TrieLevel(0), TrieLevel(1)]),
        ("E", vec![TrieLevel(1), TrieLevel(0)]),
    ]);
    // Exactly the plan worked out in join.rs's ExecutableQuery example.
    assert_eq!(level_atoms(&plan), vec![vec![0, 2], vec![0, 1], vec![1, 2]]);

    let indexes = plan.build_indexes(&db);
    assert_eq!(indexes.len(), 2, "fwd and bwd are the only distinct indexes");

    let exec = plan.bind(&indexes).expect("triangle query is non-empty");
    assert_eq!(exec.collect_dfs(), binary_triangles_directed(&edges), "triangle join");
}

// ---- planning a 2-path E(x,y) E(y,z): a single index shared by both atoms. ----
#[test]
fn test_plan_path() {
    let edges: Vec<(Value, Value)> = vec![(0, 1), (1, 2), (1, 3), (2, 3), (3, 0)];
    let db = edge_db(&edges);
    let q: Query<char, &'static str> = Query {
        vars: vec!['x', 'y', 'z'],
        atoms: vec![atom("E", &['x', 'y']), atom("E", &['y', 'z'])],
        operators: vec![],
    };
    let plan = q.plan(&['x', 'y', 'z']);

    assert_eq!(plan.atoms, vec![
        ("E", vec![TrieLevel(0), TrieLevel(1)]),
        ("E", vec![TrieLevel(0), TrieLevel(1)]),
    ]);
    assert_eq!(level_atoms(&plan), vec![vec![0], vec![0, 1], vec![1]]);

    let indexes = plan.build_indexes(&db);
    assert_eq!(indexes.len(), 1, "both atoms share one index");

    let exec = plan.bind(&indexes).expect("path query is non-empty");
    let mut want: Vec<Vec<Value>> = Vec::new();
    for &(x, y) in &edges {
        for &(y2, z) in &edges {
            if y2 == y { want.push(vec![x, y, z]); }
        }
    }
    want.sort_unstable();
    assert_eq!(exec.collect_dfs(), want, "path join");
}

// ---- planning a self-join R(x,x): the repeated variable becomes an EqColumn. ----
#[test]
fn test_plan_self_loop() {
    let db = VecDb::new().rel(
        "R", 2, vec![vec![0, 0], vec![1, 1], vec![2, 3], vec![4, 4], vec![5, 6]],
    );
    let q: Query<char, &'static str> = Query {
        vars: vec!['x'],
        atoms: vec![atom("R", &['x', 'x'])],
        operators: vec![],
    };
    let plan = q.plan(&['x']);

    assert_eq!(plan.atoms, vec![("R", vec![TrieLevel(0), EqColumn(0)])]);
    assert_eq!(level_atoms(&plan), vec![vec![0]]);

    let indexes = plan.build_indexes(&db);
    let exec = plan.bind(&indexes).expect("self-loop query is non-empty");
    assert_eq!(exec.collect_dfs(), vec![vec![0], vec![1], vec![4]], "plan self-loop");
}

// ---- an empty index sinks the whole query: bind returns None. ----
//
// R(x,x) over a relation with no diagonal rows filters to empty, so the conjunction has no
// results and bind short-circuits.
#[test]
fn test_plan_empty_index() {
    let db = VecDb::new().rel("S", 2, vec![vec![0, 1], vec![1, 0]]); // no (a,a) rows
    let q: Query<char, &'static str> = Query {
        vars: vec!['x'],
        atoms: vec![atom("S", &['x', 'x'])],
        operators: vec![],
    };
    let plan = q.plan(&['x']);
    let indexes = plan.build_indexes(&db);
    assert!(indexes.values().all(|t| t.is_none()), "the only index is empty");
    assert!(plan.bind(&indexes).is_none(), "an empty index => an empty query");
}

// ---- planning the undirected triangle E(x,y) E(y,z) E(x,z): all three atoms one index. ----
#[test]
fn test_plan_undirected_triangle() {
    let raw: Vec<(Value, Value)> = vec![
        (1, 0), (1, 2), (2, 0), (0, 3), (3, 4), (4, 0), (2, 2), (0, 1),
    ];
    let edges = to_low_high(&raw);
    let db = edge_db(&edges);
    let q: Query<char, &'static str> = Query {
        vars: vec!['x', 'y', 'z'],
        atoms: vec![atom("E", &['x', 'y']), atom("E", &['y', 'z']), atom("E", &['x', 'z'])],
        operators: vec![],
    };
    let plan = q.plan(&['x', 'y', 'z']);

    // All three atoms index (first col, second col), so they collapse to a single index.
    assert_eq!(plan.atoms, vec![
        ("E", vec![TrieLevel(0), TrieLevel(1)]),
        ("E", vec![TrieLevel(0), TrieLevel(1)]),
        ("E", vec![TrieLevel(0), TrieLevel(1)]),
    ]);
    assert_eq!(level_atoms(&plan), vec![vec![0, 2], vec![0, 1], vec![1, 2]]);

    let indexes = plan.build_indexes(&db);
    assert_eq!(indexes.len(), 1, "all three atoms share one index");

    let exec = plan.bind(&indexes).expect("undirected triangle is non-empty");
    let got = exec.collect_dfs();
    assert_eq!(got, binary_triangles_undirected(&edges), "same results as direct computation");
    assert_eq!(got, vec![vec![0, 1, 2], vec![0, 3, 4]], "expected exactly two triangles");
}

// ---- execute_bfs matches execute_dfs. ----
//
// The breadth-first executor should compute exactly the same result set as the depth-first
// one on every query. Cross-check via the full pipeline on the triangle query (shared +
// swapped tries), the path query (a trie shared across two levels), the self-loop query (a
// depth-1 EqColumn trie), and the empty-query edge case.

// Plan + bind `q` over `db`, then assert the two executors agree. (None of these queries are
// empty, so bind() should succeed.)
fn check_bfs_matches_dfs(q: &Query<char, &'static str>, order: &[char], db: &VecDb) {
    let plan = q.plan(order);
    let indexes = plan.build_indexes(db);
    let exec = plan.bind(&indexes).expect("query is non-empty");
    assert_eq!(exec.collect_dfs(), exec.collect_bfs(), "bfs and dfs disagree");
}

#[test]
fn test_bfs_matches_dfs() {
    // Directed triangle E(x,y) E(y,z) E(z,x) (shared + swapped tries).
    let tedges: Vec<(Value, Value)> = vec![
        (0, 1), (1, 2), (2, 0), (0, 2), (2, 1), (1, 0), (1, 3), (3, 1),
    ];
    let tri: Query<char, &'static str> = Query {
        vars: vec!['x', 'y', 'z'],
        atoms: vec![atom("E", &['x', 'y']), atom("E", &['y', 'z']), atom("E", &['z', 'x'])],
        operators: vec![],
    };
    check_bfs_matches_dfs(&tri, &['x', 'y', 'z'], &edge_db(&tedges));

    // Path E(x,y) E(y,z) (a single trie shared across two levels).
    let pedges: Vec<(Value, Value)> = vec![(0, 1), (1, 2), (1, 3), (2, 3), (3, 0)];
    let path: Query<char, &'static str> = Query {
        vars: vec!['x', 'y', 'z'],
        atoms: vec![atom("E", &['x', 'y']), atom("E", &['y', 'z'])],
        operators: vec![],
    };
    check_bfs_matches_dfs(&path, &['x', 'y', 'z'], &edge_db(&pedges));

    // Self-loop R(x,x) (a depth-1 EqColumn trie).
    let sdb = VecDb::new().rel(
        "R", 2, vec![vec![0, 0], vec![1, 1], vec![2, 3], vec![4, 4], vec![5, 6]],
    );
    let loop_: Query<char, &'static str> = Query {
        vars: vec!['x'],
        atoms: vec![atom("R", &['x', 'x'])],
        operators: vec![],
    };
    check_bfs_matches_dfs(&loop_, &['x'], &sdb);

    // Empty query (no variables): both should yield exactly one empty tuple.
    let empty: Query<char, &'static str> = Query { vars: vec![], atoms: vec![], operators: vec![] };
    check_bfs_matches_dfs(&empty, &[], &VecDb::new());
}
