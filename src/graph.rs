// Graph loading + binary-join reference implementations. Used by benchmarks & tests.
use std::fs::File;

use crate::hash::{Map, Set};
use crate::join::Value;
use crate::print_flush;
use crate::vec_db::VecDb;

// Build a Database with a single binary relation "E" from an edge list.
pub fn edge_db(edges: &[(Value, Value)]) -> VecDb {
    let rows: Vec<Vec<Value>> = edges.iter().map(|&(a, b)| vec![a, b]).collect();
    VecDb::new().rel("E", 2, rows)
}

// Load (a prefix of) a named dataset from data/, in sorted order. The crate directory is
// resolved at compile time, so it works regardless of the working directory. A missing
// file panics.
pub fn snap_load(dataset: &str, max_edges: Option<usize>) -> Vec<(usize, usize)> {
    let path = format!("{}/data/{dataset}", env!("CARGO_MANIFEST_DIR"));
    let file = File::open(&path).expect("could not open data file");
    println!("{dataset}: loading from {path}");
    load_edges_from(file, max_edges)
}

pub fn load_edges_from<R: std::io::Read>(source: R, max_edges: Option<usize>) -> Vec<(usize, usize)> {
    if let Some(n) = max_edges {
        print_flush!("Reading at most {n} edges.");
    } else {
        print_flush!("Reading all edges.");
    }
    use std::io::{BufRead, BufReader};
    let file = BufReader::new(source);
    let mut edges: Vec<(usize, usize)> = Vec::new();
    for readline in file.lines() {
        if max_edges.is_some_and(|n| n <= edges.len()) { break }
        let line = readline.expect("read error");
        if line.is_empty() { continue }
        if line.starts_with('#') { continue }
        let mut elts = line[..].split_whitespace();
        let v: usize = elts.next().unwrap().parse().expect("malformed src");
        let u: usize = elts.next().unwrap().parse().expect("malformed dst");
        edges.push((v, u));
    }
    print_flush!(" Got {} edges", edges.len());
    if edges.is_sorted() {
        print_flush!(", already sorted.");
    } else {
        print_flush!(", sorting...");
        edges.sort_unstable();
        print_flush!(" done.");
    }
    // Get rid of dupes. This ensures our trie-based WCOJs (which dedup implicitly) will
    // produce the same # of results as any other approach (which might not).
    print_flush!(" Deduping...");
    let before = edges.len();
    edges.dedup();
    if edges.len() == before {
        println!(" no dupes.");
    } else {
        println!(" deduped {} -> {}.", before, edges.len());
    }
    edges
}

// The directed triangle query E(x,y) E(y,z) E(z,x), via a binary join for E(x,y) E(y,z)
// followed by a hash-filter on E(z,x), then sorted.
pub fn binary_triangles_directed(edges: &[(Value, Value)]) -> Vec<Vec<Value>> {
    let edge_set: Set<(Value, Value)> = edges.iter().copied().collect();
    let mut out: Map<Value, Vec<Value>> = Map::default();
    for &(a, b) in edges { out.entry(a).or_default().push(b); }
    let mut triangles: Vec<Vec<Value>> = Vec::new();
    for &(x, y) in edges {
        if let Some(zs) = out.get(&y) {
            for &z in zs {
                if edge_set.contains(&(z, x)) { triangles.push(vec![x, y, z]); }
            }
        }
    }
    triangles.sort_unstable();
    triangles
}

// Normalize an edge list into an undirected simple graph where edges are stored oriented
// from low -> high node id. This makes every undirected triangle appear uniquely as three
// edges a -> b -> c, a -> c.
pub fn to_low_high(edges: &[(Value, Value)]) -> Vec<(Value, Value)> {
    let mut v: Vec<(Value, Value)> = edges.iter()
        .filter(|&&(a, b)| a != b)
        .map(|&(a, b)| if a < b { (a, b) } else { (b, a) })
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

// Finds undirected triangles over a low->high edge list: all {a < b < c} with edges
// a->b, b->c, a->c. Uses a binary join; sorts results.
pub fn binary_triangles_undirected(edges: &[(Value, Value)]) -> Vec<Vec<Value>> {
    let edge_set: Set<(Value, Value)> = edges.iter().copied().collect();
    let mut out: Map<Value, Vec<Value>> = Map::default();
    for &(a, b) in edges { out.entry(a).or_default().push(b); }
    let mut triangles: Vec<Vec<Value>> = Vec::new();
    for &(a, b) in edges {
        if let Some(cs) = out.get(&b) {
            for &c in cs {
                if edge_set.contains(&(a, c)) { triangles.push(vec![a, b, c]); }
            }
        }
    }
    triangles.sort_unstable();
    triangles
}
