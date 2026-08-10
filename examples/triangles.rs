// Triangle-counting benchmarks over SNAP graph datasets. Compares the WCOJ against a
// binary join with filter, and times DFS vs BFS execution.
//
//     cargo run --release --example triangles
//
// Use --release or it will be slow.
use std::rc::Rc;
use std::time::{Duration, Instant};

use rntz_joins::op::Le;
use rntz_joins::{
    binary_triangles_directed, binary_triangles_undirected, edge_db, snap_load, symmetrize,
    to_low_high, Atom, ExecutableQuery, Operator, Query, Value, VecDb,
};

// An atom over relation `rel` with the given variables.
fn atom(rel: &'static str, vars: &[char]) -> Atom<&'static str, char> {
    Atom { pred: rel, vars: vars.to_vec() }
}

fn main() {
    let datasets: Vec<&'static str> = vec![
        "ca-GrQc.txt",          // 14k undirected edges -> 48k undirected triangles
        // "wiki-Vote.txt",        // 100k -> 600k
        // "email-Enron.txt",      // 184k -> 700k
        // "soc-Slashdot0811.txt", // 470k -> 550k
        "cit-HepTh.txt",        // 350k -> 1.5m
        // "soc-Epinions1.txt",    // 400k -> 1.6m
        // "twitter_combined.txt", // 1.3m -> 13m          ~2s to run
        // "soc-LiveJournal1.txt", // 43m  -> 285m         ~2min to run!
    ];

    // With FxHash, WCO underperforms non-WCO on these (except LiveJournal1).
    // With SipHash (Rust default), it beats non-WCO except on ca-GrQc.
    // So they're competitive, but the non-WCO does more hash probes.
    println!("========== UNDIRECTED TRIANGLE BENCHMARKS ==========");
    for &name in &datasets {
        snap_triangles_undirected(name, None);
    }

    // Same canonical triangles as above, but computed by symmetrizing the graph and using
    // Le operators to filter for a < b < c, instead of pre-orienting edges low -> high.
    println!("========== OPERATOR-FILTERED TRIANGLE BENCHMARKS ==========");
    for &name in &datasets {
        snap_triangles_symmetric(name, None);
    }

    // These mostly, but not always, generate many more results. NB. each directed
    // triangle is counted 3x (for its 3 rotations), except for self-triangles (x->x->x).
    println!("========== DIRECTED TRIANGLE BENCHMARKS ==========");
    #[allow(clippy::single_element_loop)]
    for &name in &["wiki-Vote.txt"] {
        snap_triangles_directed(name, None);
    }
}

// ---- canonical (a < b < c) triangles via operators, not pre-orientation. ----
//
// Symmetrize the graph (both directions, no self-loops), then find triangles E(a,b)
// E(b,c) E(a,c) with a <= b <= c enforced by two Le operators. Without self-loops a <= b
// <= c is really a < b < c, so each undirected triangle is found exactly once: the
// canonical SNAP count, matching snap_triangles_undirected but filtering with operators
// rather than baking the orientation into the edge set. (Expect it to be slower: the edge
// set is 2x larger and the operator prunes later than pre-orientation does --
// pre-orientation prunes *before* the pick-an-atom-to-propose step.)
//
// We run the query under both operator representations to price dynamic dispatch. The
// query only uses one operator, Le, so it can be its own representation (Op = Le): the
// planner and executor monomorphize to it and the comparison should inline. The Rc<dyn
// Operator> representation instead reaches every check through a vtable.
//
// The gap turns out to be under ~1% of execute time, in either direction run to run --
// i.e. lost in the noise. It's much smaller than the penalty for running first on a cold
// cache, so we run each representation twice, in a palindrome (dyn, static, static, dyn)
// so each gets one early and one late slot, and keep its best time. Plausible reason
// there's nothing to see: the vtable target never changes, so the indirect call predicts
// perfectly, and either way the check is dwarfed by the trie's hash probes.
pub fn snap_triangles_symmetric(dataset: &str, max_edges: Option<usize>) {
    let raw = snap_load(dataset, max_edges);
    let edges = symmetrize(&raw);
    let db = edge_db(&edges);
    let rc_le = || Rc::new(Le) as Rc<dyn Operator>;

    // 1 & 2: Plan, index and execute, with dynamically and statically dispatched operators.
    let mut dynamic = symmetric_triangles(&db, rc_le());
    let mut static_ = symmetric_triangles(&db, Le);
    static_.keep_best(symmetric_triangles(&db, Le));
    dynamic.keep_best(symmetric_triangles(&db, rc_le()));

    // 3: Canonical undirected triangles via the pre-oriented binary join, as ground truth.
    let t = Instant::now();
    let want = binary_triangles_undirected(&to_low_high(&raw));
    let binary_time = t.elapsed();

    println!(
        "{dataset}: {} symmetrized edges -> {} triangles
  operator dispatch   dynamic       static     (best of 2 runs each)
  wcoj build        {:>9.2?}    {:>9.2?}
  wcoj execute      {:>9.2?}    {:>9.2?}
  wcoj total        {:>9.2?}    {:>9.2?}    found {:8} triangles
  2-edge-filter     {:>9.2?}                found {:8} triangles
",
        edges.len(), dynamic.results.len(),
        dynamic.build_time, static_.build_time,
        dynamic.exec_time, static_.exec_time,
        dynamic.total_time(), static_.total_time(), dynamic.results.len(),
        binary_time, want.len(),
    );

    assert_eq!(dynamic.results.len(), want.len(), "canonical triangle count mismatch");
    assert!(dynamic.results == want, "canonical triangle set mismatch");
    assert!(static_.results == dynamic.results, "static and dynamic dispatch disagree");
}

// One run of the symmetric triangle query: its results and how long each phase took.
struct Run {
    results: Vec<Vec<Value>>,
    build_time: Duration,       // plan + build the trie indexes
    exec_time: Duration,        // bind + execute depth-first
}

impl Run {
    fn total_time(&self) -> Duration { self.build_time + self.exec_time }

    // Keep the faster time of two runs of the same query, phase by phase.
    fn keep_best(&mut self, other: Run) {
        debug_assert!(self.results == other.results, "reruns disagree");
        self.build_time = self.build_time.min(other.build_time);
        self.exec_time = self.exec_time.min(other.exec_time);
    }
}

// Plan, index and run E(a,b) E(b,c) E(a,c) with a <= b <= c, representing the two Le
// operators as `Op`. Instantiating this at different `Op`s is the whole point: everything
// downstream of the Query -- plan(), Level<Op>, the executor's proposer/filter calls -- is
// generic in the operator representation, so each instantiation dispatches its own way.
fn symmetric_triangles<Op: Operator + Clone>(db: &VecDb, le: Op) -> Run {
    let q: Query<char, &'static str, Op> = Query {
        vars: vec!['a', 'b', 'c'],
        atoms: vec![atom("E", &['a', 'b']),
                    atom("E", &['b', 'c']),
                    atom("E", &['a', 'c'])],
        operators: vec![Atom { pred: le.clone(), vars: vec!['a', 'b'] },   // a <= b
                        Atom { pred: le, vars: vec!['b', 'c'] }],          // b <= c
    };

    let t = Instant::now();
    let plan = q.plan(&['a', 'b', 'c']);
    let indexes = plan.build_indexes(db);
    let build_time = t.elapsed();

    let t = Instant::now();
    let exec = plan.bind(&indexes).expect("triangle query is non-empty");
    let results = run_plan(&exec);
    Run { results, build_time, exec_time: t.elapsed() }
}

// Run a plan depth-first, collect results, and sort. Like ExecutableQuery::collect_dfs but with a
// progress print every million results, since the big datasets take a while.
fn run_plan<Op: Operator>(plan: &ExecutableQuery<Op>) -> Vec<Vec<Value>> {
    let mut out: Vec<Vec<Value>> = Vec::new();
    let mut counter: usize = 0;
    plan.execute_dfs(|row| {
        out.push(row.to_vec());
        counter += 1;
        if counter.is_multiple_of(1_000_000) {
            println!("found {:2} million results!", counter / 1_000_000);
        }
    });
    out.sort_unstable();
    out
}

// ---- triangle query on a real SNAP dataset. ----
//
// Loads (a prefix of) the named dataset from data/ and runs the same triangle query as the
// unit tests, cross-checked against brute force. `max_edges` caps how much of the file we
// read so we can start small and scale up; None means "the whole file".
#[allow(dead_code)]
pub fn snap_triangles_directed(dataset: &str, max_edges: Option<usize>) {
    let edges = snap_load(dataset, max_edges);
    let db = edge_db(&edges);

    let q: Query<char, &'static str> = Query {
        vars: vec!['x', 'y', 'z'],
        atoms: vec![atom("E", &['x', 'y']),
                    atom("E", &['y', 'z']),
                    atom("E", &['z', 'x'])],
        operators: vec![],
    };

    // 1: Plan and build the trie indexes.
    let wcoj_start = Instant::now();
    let plan = q.plan(&['x', 'y', 'z']);
    let indexes = plan.build_indexes(&db);
    let build_time = wcoj_start.elapsed();

    // 2: Execute the join.
    let t = Instant::now();
    let exec = plan.bind(&indexes).expect("triangle query is non-empty");
    let got = run_plan(&exec);
    let exec_time = t.elapsed();
    let total_time = wcoj_start.elapsed();

    // 3: Binary join for comparison.
    let t = Instant::now();
    let want = binary_triangles_directed(&edges);
    let binary_time = t.elapsed();

    println!(
        "{dataset}: {} undirected edges -> {} triangles
  wcoj build    {:>9.2?}
  wcoj execute  {:>9.2?}
  wcoj total    {:>9.2?}    found {:8} triangles
  2-edge-filter {:>9.2?}    found {:8} triangles
",
        edges.len(), got.len(),
        build_time,
        exec_time,
        total_time, got.len(),
        binary_time, want.len(),
    );

    // There are too many triangles to print on mismatch, so just compare counts first
    // (a nicer message than a full set diff) and then the full sets.
    assert_eq!(got.len(), want.len(), "triangle count mismatch");
    assert!(got == want, "triangle set mismatch");
}

// ---- undirected triangle count (matches SNAP's published figures). ----
//
// Reorient edges low->high and dedup, so each undirected triangle {a<b<c} shows up as
// a->b, b->c, a->c exactly once. The query is therefore E(x,y) E(y,z) E(x,z) (note the
// last atom, vs E(z,x) for directed 3-cycles), order x,y,z.
pub fn snap_triangles_undirected(dataset: &str, max_edges: Option<usize>) {
    let raw = snap_load(dataset, max_edges);
    let edges = to_low_high(&raw);
    let db = edge_db(&edges);

    let q: Query<char, &'static str> = Query {
        vars: vec!['x', 'y', 'z'],
        atoms: vec![atom("E", &['x', 'y']),
                    atom("E", &['y', 'z']),
                    atom("E", &['x', 'z'])],
        operators: vec![],
    };

    // 1: Plan and build the trie indexes.
    let wcoj_start = Instant::now();
    let plan = q.plan(&['x', 'y', 'z']);
    let indexes = plan.build_indexes(&db);
    let build_time = wcoj_start.elapsed();

    // 2a: Execute the join depth-first.
    let t = Instant::now();
    let exec = plan.bind(&indexes).expect("undirected triangle query is non-empty");
    let got = run_plan(&exec);
    let exec_time = t.elapsed();
    let total_time = wcoj_start.elapsed();

    // 2b: Execute the join breadth-first.
    let t = Instant::now();
    let got_bfs = exec.collect_bfs();
    let bfs_time = t.elapsed();

    // 3: Binary join for comparison.
    let t = Instant::now();
    let want = binary_triangles_undirected(&edges);
    let binary_time = t.elapsed();

    println!(
        "{dataset}: {} undirected edges -> {} triangles
  wcoj build    {:>9.2?}
  wcoj exec dfs {:>9.2?}    found {:8} triangles
  wcoj exec bfs {:>9.2?}    found {:8} triangles
  wcoj total    {:>9.2?}    (build + dfs)
  2-edge-filter {:>9.2?}    found {:8} triangles
",
        edges.len(), got.len(),
        build_time,
        exec_time, got.len(),
        bfs_time, got_bfs.len(),
        total_time,
        binary_time, want.len(),
    );

    assert_eq!(got.len(), want.len(), "undirected triangle count mismatch");
    assert!(got == want, "undirected triangle set mismatch");
    assert!(got == got_bfs, "bfs vs dfs triangle set mismatch");
}
