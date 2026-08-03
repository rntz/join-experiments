## Issues

**2. `execute_dfs` panics on a constant-only atom mixed with variable atoms (`:400-403`).**
```rust
tries: self.tries.iter().map(|&t| match t {
    Trie::Node(map) => map,
    Trie::Leaf => unreachable!(),   // <-- fires for a depth-0 trie
}).collect(),
```
A fully-constant atom like `T(5)` builds `Some(Trie::Leaf)` (depth 0) and contributes 0 entries to `levels`. The pure all-constants case is safely handled by the `levels.len()==0` early return, but a *mixed* query — e.g. `E(x,y) T(5)` — has non-empty `levels` **and** a `Leaf` in `tries`, so this `unreachable!()` becomes reachable and panics. No such query is built today, and the "desugar constants into singleton relations" note (`:329`) would avoid it, but it's a sharp edge a future planner must respect.

**3. Empty index → can't even build the plan (`:356-359`, the existing TODO).**
`tries: Vec<&'a Trie>` can't represent `None`, but `Trie::build` returns `None` for an empty relation. Any query where one atom's index is empty (answer: zero results) instead panics at the `.unwrap()` on the build call. You've flagged this; noting it as the highest-value TODO since it turns a legitimate query into a crash.

**4. `min_by_key(...).expect()` panics on a zero-width level (`:439-441`).**
A `levels[i]` that is empty (a variable in no atom) yields `(0..0).min_by_key(..)` = `None` → `.expect()` panic. Only reachable via a malformed plan, but worth an `assert!`/comment stating the "every variable appears in ≥1 atom" invariant explicitly.
