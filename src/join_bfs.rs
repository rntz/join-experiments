// Breadth-first search version of WCOJ execution, Claude-generated. Didn't outperform DFS
// significantly so I'm not investigating further yet. I think it would need further
// tweaking to incorporate the insights from DataToad; see "MICHAEL NOTES" below.
use crate::join::{Value, QueryPlan, TrieMap, Trie};

impl<'a> QueryPlan<'a> {
    // ======================================================================
    // BELOW IS CLAUDE-GENERATED CODE THAT I HAVE NOT REVIEWED YET - MICHAEL
    // ======================================================================
    //
    // Execute breadth-first, à la Frank McSherry's DataToad (see the "COMPUTATIONAL ATOMS"
    // aside above). Instead of backtracking one variable at a time, we maintain a *frontier*
    // of partial solutions for the first k variables and extend the whole frontier to k+1
    // variables at each step. This is the same proposer/intersect logic as execute_dfs, just
    // scheduled level-by-level over all partial solutions rather than depth-first over one.
    //
    // The frontier is stored columnar (row-major flat vectors) to avoid a heap allocation
    // per partial solution:
    //  - `prefixes`: `depth` values per solution, where depth = # variables assigned so far.
    //  - `nodes`: `n_atoms` trie-node pointers per solution (each atom's current trie node).
    // So partial solution `row` owns prefixes[row*depth .. (row+1)*depth] and
    // nodes[row*n_atoms .. (row+1)*n_atoms].
    pub fn execute_bfs<F>(&self, mut f: F) where F: FnMut(&[Value]) {
        let n_vars = self.levels.len();
        if n_vars == 0 { // mirror execute_dfs's empty-query case.
            f(&[]);
            return;
        }
        let n_atoms = self.tries.len();

        // Initial frontier: a single partial solution with an empty prefix and every atom
        // sitting at its root node.
        let mut prefixes: Vec<Value> = Vec::new();
        let mut nodes: Vec<&TrieMap> = self.tries.iter().map(|&t| match t {
            Trie::Node(map) => map,
            Trie::Leaf => unreachable!(),
        }).collect();

        // Reused scratch: children at the current level, and the emitted-row buffer.
        let mut children: Vec<&Trie> = Vec::new();
        let mut out: Vec<Value> = Vec::with_capacity(n_vars);

        for (level_idx, level) in self.levels.iter().enumerate() {
            let depth = level_idx;                // prefix length of the current frontier
            let count = nodes.len() / n_atoms;    // # partial solutions in the frontier
            let width = level.len();
            let last = level_idx + 1 == n_vars;

            // The next frontier we're building (unused on the last level, where we emit).
            let mut next_prefixes: Vec<Value> = Vec::new();
            let mut next_nodes: Vec<&TrieMap> = Vec::new();

            // ---------- MICHAEL NOTES ----------
            //
            // I don't think this is actually similar to datatoad's approach. It hasn't
            // separated the count/propose step from the filter step. This means that
            // instead of each filter going to town over a huge slice of data, it only
            // chews on the results of a single proposer.
            //
            // Maybe download Frank's blog post, point Claude at it, and ask it to
            // redesign?
            //
            // Might be worth it just to see whether this actually improves performance!
            // Although, in Frank's case, it might mostly be about minimizing dynamic
            // dispatch; there is dispatch in this code but it's from a very small # of
            // options.
            //
            // Also, I might not see any perf improvements unless I can get Rust to
            // vectorize the filter kernels. I should probably do some isolated
            // vectorization experiments to see how hard it is to do this and what kind of
            // speedups I can get.
            //
            // ---------- END MICHAEL NOTES ----------
            for row in 0..count {
                let pfx = &prefixes[row * depth..row * depth + depth];
                let row_nodes = &nodes[row * n_atoms..row * n_atoms + n_atoms];

                // The proposer is the atom in this level with the fewest children.
                let proposer_pos = (0..width)
                    .min_by_key(|&pos| row_nodes[level[pos]].len())
                    .expect("Empty level - every query variable must be used in some atom!");
                let proposer_map = row_nodes[level[proposer_pos]];

                'keys: for (key, child) in proposer_map {
                    // Intersect: look up this key in every other trie at this level.
                    children.clear();
                    for pos in 0..width {
                        if pos == proposer_pos { children.push(child); continue; }
                        match row_nodes[level[pos]].get(key) {
                            Some(c) => children.push(c),
                            None => continue 'keys,
                        }
                    }

                    // A match. On the last variable, emit; otherwise extend the frontier.
                    if last {
                        out.clear();
                        out.extend_from_slice(pfx);
                        out.push(*key);
                        f(&out);
                    } else {
                        next_prefixes.extend_from_slice(pfx);
                        next_prefixes.push(*key);
                        // Copy this solution's nodes, then descend the atoms in this level to
                        // their child under `key`. A Leaf child bottoms out and is never read.
                        let base = next_nodes.len();
                        next_nodes.extend_from_slice(row_nodes);
                        for (pos, &trie_idx) in level.iter().enumerate() {
                            if let Trie::Node(map) = children[pos] { next_nodes[base + trie_idx] = map; }
                        }
                    }
                }
            }

            prefixes = next_prefixes;
            nodes = next_nodes;
        }
    }
    // ======================================================================
    // END UNREVIEWED LLM GENERATED CODE
    // ======================================================================

    // As collect_dfs, but breadth-first. Lets callers compare the two execution strategies.
    pub fn collect_bfs(&self) -> Vec<Vec<Value>> {
        let mut out: Vec<Vec<Value>> = Vec::new();
        self.execute_bfs(|row| out.push(row.to_vec()));
        out.sort_unstable();
        out
    }
}
