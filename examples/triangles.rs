// Triangle-counting benchmarks over SNAP graph datasets. Compares the WCOJ plan (built by
// hand, since query planning doesn't exist yet) against a brute-force binary join, and
// times DFS vs BFS execution.
//
//     cargo run --release --example triangles
//
// Use --release or it will be slow.
use std::time::Instant;

use rntz_joins::IndexColumnShape::TrieLevel;
use rntz_joins::{
    binary_triangles_directed, binary_triangles_undirected, edge_db, snap_load, to_low_high,
    QueryPlan, Trie, Value,
};

fn main() {
    let datasets: Vec<&'static str> = vec![
        "ca-GrQc.txt",          // 14k undirected edges -> 48k undirected triangles
        "wiki-Vote.txt",        // 100k -> 600k
        "email-Enron.txt",      // 184k -> 700k
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

    // // These mostly, but not always, generate many more results. NB. each directed
    // // triangle is counted 3x (for its 3 rotations), except for self-triangles (x->x->x).
    // println!("========== DIRECTED TRIANGLE BENCHMARKS ==========");
    // for &name in &datasets {
    //     snap_triangles_directed(name, None);
    // }
}

// Run a plan depth-first, collect results, and sort. Like QueryPlan::collect_dfs but with a
// progress print every million results, since the big datasets take a while.
fn run_plan(plan: &QueryPlan) -> Vec<Vec<Value>> {
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

    // WCOJ phase 1: build the trie indexes.
    let wcoj_start = Instant::now();
    let fwd = Trie::build(&db, "E", &vec![TrieLevel(0), TrieLevel(1)]).unwrap();
    let bwd = Trie::build(&db, "E", &vec![TrieLevel(1), TrieLevel(0)]).unwrap();
    let build_time = wcoj_start.elapsed();

    // WCOJ phase 2: execute the join, materializing + sorting the results just like the
    // brute force does, so the two are compared on equal terms.
    let plan = QueryPlan {
        tries: vec![&fwd, &fwd, &bwd],
        levels: vec![vec![0, 2], vec![0, 1], vec![1, 2]],
    };
    let t = Instant::now();
    let got = run_plan(&plan);
    let exec_time = t.elapsed();
    let total_time = wcoj_start.elapsed();

    let t = Instant::now();
    let want = binary_triangles_directed(&edges);
    let brute_time = t.elapsed();

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
        brute_time, want.len(),
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

    let wcoj_start = Instant::now();
    let fwd = Trie::build(&db, "E", &vec![TrieLevel(0), TrieLevel(1)]).unwrap();
    let build_time = wcoj_start.elapsed();

    let plan = QueryPlan {
        tries: vec![&fwd, &fwd, &fwd],
        levels: vec![vec![0, 2], vec![0, 1], vec![1, 2]],
    };
    let t = Instant::now();
    let got = run_plan(&plan);
    let exec_time = t.elapsed();
    let total_time = wcoj_start.elapsed();

    // Same plan, breadth-first, so we can compare the two execution strategies.
    let t = Instant::now();
    let got_bfs = plan.collect_bfs();
    let bfs_time = t.elapsed();

    let t = Instant::now();
    let want = binary_triangles_undirected(&edges);
    let brute_time = t.elapsed();

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
        brute_time, want.len(),
    );

    assert_eq!(got.len(), want.len(), "undirected triangle count mismatch");
    assert!(got == want, "undirected triangle set mismatch");
    assert!(got == got_bfs, "bfs vs dfs triangle set mismatch");
}
