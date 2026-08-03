Claude-generated suggestions below. Claude is very wordy, here's my summary:

- On wasm there's no good way to do prefetching. This means we should try to minimize
  cache misses total, not work around them through parallelism. Memory locality is
  therefore very important.

- A columnar sorted representation might therefore be better than nesting hashtables.

- Hoisting dispatch out of hot loops matters a lot on WASM (more than native), because
  apparently wasm needs to do a bounds check and *runtime argument type check* on every
  dynamic call! (We're not currently doing any dynamic dispatch but computed atoms might
  want it.)

- Allocating stuff in an "arena" might be better than relying on Rust's default allocator
  on WASM, by collecting large allocations into one big allocation. (Claude describes its
  specific ideas for how to do this at bottom of file.) However, this is contingent on
  moving away from hashtables in trie nodes (since hashtables require an indirect
  allocation) and instead using small sorted vectors. This in turn is problematic for
  in-place updates, so I think arena-fying tries is probably a bad choice.

I'm going to stick with the DFS nested-hash-trie approach for now and worry about
performance once it's actually a problem. I don't know how much weight/trust to give these
suggestions (for instance, prodding revealed Claude made a mistake in thinking using u32s
would save memory on wasm when it doesn't; usize = u32 = pointer size on wasm).

---------- CLAUDE'S SUGGESTIONS BELOW ----------

# WCOJ performance suggestions for a WebAssembly-only target

*Written 2026-08-02. Scope: **WebAssembly is the only intended target; native
performance does not matter.** That constraint drives every conclusion below —
several of them invert what you'd conclude for native.*

Some wasm-spec claims (dropped prefetch proposal, one-way `memory.grow`, no
gather in SIMD) reflect knowledge as of early 2026 — worth re-verifying against
the current proposals if you're about to build on them, but don't plan around
them changing.

---

## TL;DR — what to actually do

Ranked by expected payoff on wasm, highest first:

1. **Arena-flatten the trie.** Replace the nested per-node `HashMap<Value, Trie>`
   (`src/join.rs:130`) with a `u32`-indexed arena. Biggest, lowest-risk win.
2. **Switch the index representation to sorted-column + galloping/leapfrog**
   (the dijkstralog direction already sketched in `src/join.rs:136`). This is the
   *primary* strategy on wasm, not a someday-maybe.
3. **Keep `execute_dfs` as the executor for now.** Do *not* rewrite `execute_bfs`
   into DataToad shape yet — on hash-tries, on wasm, it's a net-negative move.
4. **Defer the DataToad re-orientation** until you add computational/heterogeneous
   atoms, and when you do it, do it as **bounded-batch (morsel) columnar**, never
   full-frontier materialization.
5. Keep hot loops **monomorphized** — no trait objects in the inner loop.
6. **If the indexes will be mutated for incremental maintenance:** represent diffs
   as **(add, remove) set-pairs (count-free)** and compute change by evaluating
   *derivative queries* on the engine you already have — see [Incremental
   maintenance](#incremental-maintenance-ivm-if-the-indexes-will-be-mutated). Do
   *not* reach for Z-set counts by default; they only pay off when deletions
   through projection/`∃` are a hot path.

The rest of this document explains why.

---

## Governing thesis: on wasm you can't hide misses, so have fewer of them

The dominant cost in this join is random-access cache misses — hash probes into
trie nodes that don't fit in cache. On **native**, the plan was to *hide* that
latency: use software prefetch to overlap many independent probes (memory-level
parallelism). On **wasm**, every tool for hiding latency is unavailable:

- **No prefetch instruction.** Core wasm has none. A SIMD-era proposal for one
  was dropped. Portable prefetch intrinsics (`llvm.prefetch`) lower to a **no-op**
  on wasm; arch-specific ones (`_mm_prefetch`, aarch64 `_prefetch`) aren't even in
  scope for the `wasm32` target.
- **No gather/scatter** in wasm SIMD (128-bit fixed width; relaxed-SIMD doesn't
  add general gather), so you can't vectorize the probes either.
- **Weaker instruction scheduling.** You're compiled by Liftoff→TurboFan or
  Cranelift, not LLVM. The native fallback of "cluster independent loads and let
  the out-of-order engine overlap them" is both less likely to survive codegen and
  still capped by the reorder-buffer depth.

Conclusion: the winning strategy shifts from *hiding* misses (prefetch, MLP,
gather — all denied) to *having fewer, more predictable* misses. That is a
**locality-first** agenda: flatten allocations and go sequential/sorted. Every
recommendation below follows from this.

---

## Background: is `execute_bfs` analogous to DataToad? No.

The "MICHAEL NOTES" comment in `src/join.rs:421` is **correct**. The current
`execute_bfs` (`src/join.rs:391`) is best described as *level-synchronized DFS*:
it materializes the frontier level-by-level instead of recursing, but the inner
work is the same shape as `QueryDfsState::execute` (`src/join.rs:515`). Compare
the inner loops (`src/join.rs:442` vs `src/join.rs:529`) — same proposer-pick +
intersect, just fed from a flat frontier.

Frank McSherry's DataToad (see `frank-mcsherry-2025-12-23.md`, esp. the loop
listing) re-orients into **three separate passes per variable — count, propose,
validate — each with atoms outermost and bindings innermost**:

```
for each variable:
    for each atom: for each binding: COUNT
    for each atom: for each binding: PROPOSE
    for each atom: for each binding: VALIDATE
```

The current code instead keeps **bindings outermost** (`for row in 0..count`)
and **fuses** count (`min_by_key`), propose (iterate the proposer map), and
validate (`.get(key)`) together per binding. So a validate step never sees more
than one proposer's keys for one binding — exactly the "it only chews on the
results of a single proposer" the note calls out. The note is accurate.

---

## Why the re-orientation looked attractive on native — and why that reason is gone

Frank's re-orientation has two real payoffs:

1. **Hoisting dynamic dispatch** out of the inner loop (its headline motivation) —
   pays off only with *heterogeneous* atoms (data vs. code / computational atoms).
2. **Vectorization** — pays off only with a *sorted-columnar* representation whose
   kernels vectorize.

Neither holds for the current homogeneous, hash-trie triangle workload: atoms are
one concrete `Trie` enum (a cheap branch, no dispatch), and hash probes don't
vectorize.

On **native**, there was a third path that *would* have justified the rewrite:
the re-orientation produces a materialized candidate column, and a column of
independent probes is exactly what **group prefetching** (AMAC-style) needs to
overlap miss latency. That was the one path expected to beat DFS.

**On wasm that path does not exist** (no prefetch — see thesis above). Remove it
and the re-orientation, applied to the current hash-tries, is left with only
costs: extra passes over the frontier, intermediate count/candidate columns,
worse temporal locality, and — new on wasm — a memory-footprint problem (next
section). **So re-orienting on top of the hash-tries is a net-negative move on
wasm.** Don't do it for its own sake.

The one surviving motivation — hoisting dispatch for computational atoms —
actually gets *stronger* on wasm, because dispatch there is `call_indirect`
(table-bounds + type-signature check), pricier relative to the work than a native
indirect call. But it's contingent on such atoms existing; it does nothing for the
triangle workload.

---

## What wasm changes, concretely

| Factor | Native | wasm | Consequence |
|---|---|---|---|
| Software prefetch | yes | **none** (no-op) | Can't hide probe latency → locality-first |
| SIMD gather/scatter | yes (AVX2+) | **none** | Can't vectorize hash probes |
| SIMD width | up to 512-bit | **128-bit fixed** | Only modest kernel vectorization, sorted-column only |
| Scheduler quality | LLVM | Liftoff/TurboFan/Cranelift | Weak-MLP fallback unreliable |
| Indirect call | cheap-ish | `call_indirect` (checked) | Dispatch hoisting matters *more* — but only with hetero atoms |
| `usize` width | 64-bit | **32-bit** on `wasm32` | `Value`/indices are already compact; prefer `u32`, avoid gratuitous `i64` |
| Allocator | fast, syscall-backed | dlmalloc **inside** linear memory | Allocation churn is real wasm work → arena wins more |
| `memory.grow` | n/a | effectively **one-way** (no `memory.shrink`) | Peak footprint sticks; wide frontiers are dangerous |

---

## Wasm-specific caution: full-frontier BFS is a memory liability

`execute_bfs` materializes the **entire** frontier at each level (`prefixes` /
`nodes`, `src/join.rs:401`). For a real triangle input that's millions of partial
solutions. On wasm this matters more than on native because:

- `memory.grow` is effectively one-way — there's no `memory.shrink` in the MVP, so
  the peak linear-memory footprint sticks for the module's lifetime; and
- you're inside a 4 GB (`wasm32`) address space, often with a browser-imposed cap
  well below that.

`execute_dfs` keeps a bounded `O(#vars)` working set and doesn't have this problem.
So if you ever do go columnar, process the frontier in **fixed-size batches
("morsels")** — extend a bounded chunk of partial solutions at a time, not the
whole frontier. Batching also keeps the working set L2-resident, which is the
substitute for the prefetch you don't have.

---

## The levers, re-ranked for wasm

### 1. Arena-flatten the trie (biggest win, do first)

Today each trie node is a separate `HashMap<Value, Trie>` allocation
(`src/join.rs:130`); execution pointer-chases through random cache lines — exactly
the latency wasm can't hide. Flatten to a `u32`-indexed arena of nodes.

Why it's unusually good on wasm:
- Contiguity + no hashmap overhead: packed arrays keep related nodes on adjacent
  cache lines and drop hashbrown's per-node handle (~16 bytes on `wasm32`), its
  bucket slack, and control bytes. (A `u32` index is *not* smaller than a wasm
  pointer — both are 32-bit offsets into linear memory — so the win is locality and
  allocations, not reference size.)
- dlmalloc runs *as wasm code inside linear memory*; collapsing thousands of
  per-node allocations into one arena removes real, measurable work.
- Helps every executor (DFS or a future columnar one) and is low-risk.
- If IVM is in scope, the arena must also support **delete**, not just
  `Trie::build` (`src/join.rs:201`) — this constrains the arena flavor; see
  [Incremental maintenance](#incremental-maintenance-ivm-if-the-indexes-will-be-mutated).

### 2. Sorted-column + galloping representation (primary strategy)

The dijkstralog direction already discussed in `src/join.rs:136`. Its win is
sequential access and no hashing — *fewer and more predictable misses* — which is
precisely what wasm rewards and can't otherwise get. It's also the only
representation that can use the SIMD wasm *does* have: 128-bit compares,
`i8x16.swizzle` (in-register 16-byte shuffle), and lane bitmask extraction can
accelerate merge/gallop kernels. Hash probes get none of that.

The case for sorted-over-hash is *stronger* on wasm than native: native could have
masked hashing's random access with prefetch; wasm cannot. Note hashing arithmetic
itself (FxHash: rotate/xor/mul, `src/hash.rs`) is fine on wasm — the problem is the
random-access pointer-chasing it drives, not the hash computation.

### 3. Minimize dynamic dispatch

Keep the inner loop monomorphized; avoid trait objects in the hot path so you don't
pay `call_indirect`. Relevant now, and a prerequisite for doing computational atoms
efficiently later.

### 4. (Conditional) DataToad re-orientation — only with computational atoms, only bounded-batch

When you add computational/heterogeneous atoms, revisit the re-orientation: the
dispatch-hoisting is worth *more* on wasm than native. But implement it as
bounded-batch columnar (see the memory caution above), not full-frontier.

---

## Incremental maintenance (IVM): if the indexes will be mutated

Everything above assumed build-once / read-many. If base relations change and you
maintain query results incrementally, the index must support **delete**, not just
`Trie::build` (`src/join.rs:201`). Two decisions follow: a *representation*
decision (does the arena survive mutation?) and an *algebra* decision (how are
diffs represented?). **Scope:** this assumes **non-recursive** queries — no
maintained fixpoints — matching the part of the theory we're drawing on.

### Does the arena (lever #1) survive mutation? Yes — but not as one immutable CSR blob

"Arena-flatten" bundled three separable decisions; mutation constrains only the
last two:

1. **Reference discipline** — `u32` arena index vs. pointer/`Box`. Fully compatible
   with mutation, still wanted on wasm (`usize` is `u32` on `wasm32`). Keep it.
2. **Per-node container** — hash map vs. sorted vec vs. CSR slice. Sorted/CSR need
   shifting to insert.
3. **Backing-store discipline** — per-node alloc vs. slab+freelist vs.
   immutable-batch-and-merge.

The combination that *doesn't* work is **one monolithic contiguous CSR arena +
in-place point updates** (inserting a child shifts the tail and renumbers indices,
O(n) per insert). Other flavors are fine:

- **Batched rebuild** — keep the read-optimized arena; rebuild affected sub-tries
  per delta batch. *Wasm caveat:* rebuild **into the same backing `Vec`** (clear +
  refill), never allocate-fresh-while-old-is-live — `memory.grow` is one-way, so a
  transient double footprint never comes back.
- **Slab / generational arena** (stable indices + freelist, à la `slotmap`) — O(1)
  insert/delete, but freed slots fragment and erode the locality you flattened for,
  so you need periodic compaction anyway.

Batched, sequential rebuild/merge is the wasm-friendly regime. In-place point
mutation is the *opposite* of the locality-first thesis — random access you can't
prefetch — so on wasm, prefer **batched deltas**.

### Diff representation: (add, remove) set-pairs, not counts

Two established algebras:

- **Z-sets / weights** (DBSP — Budiu, McSherry, Ryzhyk, Tannen, VLDB 2023): each
  tuple carries an integer weight; deletion is a −1; a tuple is gone at weight 0.
  Merges are integer addition — clean, commutative, SIMD-friendly — which is why
  mature *arrangement/LSM* systems use it.
- **(add, remove) set-pairs** (`fixing-incremental-computation.pdf` — Alvarez-
  Picallo, Eyers-Taylor, Peyton Jones, Ong, 2018): a change is a disjoint pair
  `(p, q)` applied as `a ⊕ (p,q) = (a ∨ p) ∧ ¬q`; the precise diff of two states is
  `(a ∧ ¬b, b ∧ ¬a)` = `(added, removed)`. **No per-tuple counts.**

For our scope (non-recursive; `∃` and `¬`; no maintained fixpoints) set-pairs are
the better fit, for a reason specific to this engine:

**IVM becomes "run more queries on the engine you already have."** The paper's
derivative is two formula transformers — Δ (additions) and ∇ (removals), Fig. 2
p12 — whose outputs are themselves conjunctive/disjunctive/projected/negated
formulae, i.e. WCOJ queries. Maintenance = (1) apply `(add, remove)` diffs to the
base indexes, (2) evaluate the Δ/∇ output-delta formulae against the *updated*
indexes. No separate weighted-arrangement subsystem; no per-tuple weight storage.

Key rules and their cost:
- **Join:** `Δ(T ∧ U) = (ΔT ∧ X(U)) ∨ (ΔU ∧ X(T))` — the semi-naïve delta-join, a
  small (delta) side against the updated other side. Fast under WCOJ because the
  delta drives enumeration.
- **Negation:** `Δ(¬T) = ∇(T)`, `∇(¬T) = Δ(T)`. You never materialize the
  complement (the whole schema universe) — only its *changes*, which are small.
  This is what makes `¬` maintainable, and it dovetails with the
  non-materializable-atom direction the `join.rs` comments already gesture at (the
  `equal` / `NotEq`-style relations).
- **Existential / projection deletion (the one hard case):**
  `∇(∃x.T) = ∃x.∇(T) ∧ ¬∃x.X(T)`. A projected tuple leaves only if a witness was
  removed *and* none survives in the updated relation. That `¬∃x.X(T)` is a
  **re-query of the updated index** — exactly the job a derivation count does in
  O(1). (Paper footnote 12 names the counting alternative — the "support"
  structures of DRed — explicitly; this is the count-free substitute.)

### Correcting the earlier "you need counts" claim

Scoped to non-recursive maintenance, IVM does **not** require Z-set weights:
- **Structural pruning** of an emptied trie node is an `is_empty()` check after
  removal — not a semantic count. (An earlier turn suggested per-node live-child
  counts; those aren't needed.)
- **Correct deletion** through duplicates/projection is handled by set-pairs +
  re-query, not derivation counts.

So don't reach for a weighted arrangement by default. Counts earn their keep only
when **deletions through projection / `∃` / aggregation are a hot path** — there
the choice is counts (O(1)-local, +memory, +LSM machinery) vs. set-pair re-query
(bounded extra probes, −memory). **If your maintained queries don't project** (all
join variables retained — e.g. triangle *enumeration*, where each output tuple has
a unique support), deletion is unambiguous and counts buy nothing; set-pairs are
strictly better.

### Wasm-specific consequences

- **Memory:** set-pairs store no per-tuple weight — a real plus under one-way
  `memory.grow` and the 4 GB cap, and skipping the arrangement subsystem is less
  code and less resident state.
- **The `∇(∃x.T)` re-query** is extra random-access probing on the deletion path —
  the pattern wasm can't prefetch. It's delta-bounded and batchable (just another
  join), so acceptable *unless* deletion-through-projection dominates; if it does,
  reconsider counts for those relations specifically.
- **Precision knob (§4.2.3):** the derivative isn't always precise — cases marked †
  (`Δ(T∨U)`, `Δ(∃x.T)`) can re-derive already-present tuples. Correct (idempotent
  under `∨`) but wasteful, and on wasm wasted work = extra unhideable probes. The
  precise variants cost extra negated conjuncts; lean slightly more toward them on
  wasm than you would on native.

---

## Suggested sequencing

1. Arena-flatten the trie to `u32` indices. Re-benchmark `triangles`
   (`examples/triangles.rs`) — expect a real improvement from locality +
   allocation reduction alone, with no algorithm change.
2. Prototype the sorted-column + galloping representation; benchmark against the
   arena hash-trie. Add 128-bit SIMD to the merge kernel where it helps.
3. Keep DFS throughout steps 1–2. Only reconsider a columnar/BFS executor once
   computational atoms are on the table, and then only bounded-batch.
4. Throughout: prefer `u32`, avoid `i64`, keep the hot path monomorphized.

## What *not* to spend time on (for wasm)

- Group/software prefetching — no instruction exists; it's a no-op.
- Wide-SIMD or gather-based probe kernels — wasm SIMD is 128-bit with no gather.
- Rewriting `execute_bfs` into DataToad shape on the current hash-tries — costs
  (extra passes, intermediate columns, worse locality, unbounded frontier memory)
  with none of the native-only prefetch payoff.

---

## Open questions / to verify before building

- Re-confirm the wasm prefetch and `memory.shrink` situation against current
  proposals (both were absent as of early 2026).
- Which wasm runtime is the deployment target (browser V8/SpiderMonkey vs.
  Wasmtime/Cranelift vs. LLVM-AOT via emscripten)? Codegen quality varies and
  affects how much the weak-MLP fallback is worth.
- Is there a memory cap in the deployment environment? It bounds how large a
  frontier (or arena) you can afford and sharpens the DFS-vs-columnar tradeoff.
- Do you expect >4 G distinct interned values? If so, `wasm32`'s 32-bit `usize`
  forces `memory64` or a wider index type — decide early, it touches the arena.
- IVM scope: will maintained queries **project** (use `∃` / drop join variables) or
  aggregate, and are deletions frequent? That decides whether count-free set-pairs
  suffice or whether counts pay off for specific relations. Also: what's the delta
  batch size (batched deltas favor rebuild/merge; unbatchable point updates push
  toward a mutable structure)?

---

## Appendix: what "arena" means and how it would represent the trie

An "arena" is a single owner of many objects: instead of each node owning its
children through nested ownership and separate heap allocations, you put **all
nodes in one `Vec`, and refer to a node by its index (`u32`) into that `Vec`**
instead of by pointer. "Allocate a node" becomes "push onto the Vec and return its
index." It's a reference/ownership discipline, not a data structure per se.

### What the trie is now

```rust
pub enum Trie { Leaf, Node(Map<Value, Trie>) }   // Map = HashMap<Value, Trie, Fx>
```

The root is a hashmap; each entry's value is a `Trie` stored inline, and a
`Node`'s inner map has its **own heap buffer** elsewhere. So the trie is a spray of
separately-allocated hashmaps linked by ownership. Descending one level =
hash-probe the current map → get an inline `Trie` → if it's a `Node`, its map lives
at some unrelated heap address (a pointer chase to a random cache line).

### Minimal arena: nodes in a Vec, children by index

```rust
struct Arena { nodes: Vec<NodeMap> }   // nodes[0] is the root
type NodeMap = Map<Value, u32>;        // Value -> index of child node in `nodes`
```

Descend: `let child = arena.nodes[cur].get(&k)?; cur = *child;`

What this buys: nodes are relocatable/contiguous and there's no boxed-enum chase.
(The `u32` child links are *not* smaller than pointers — on `wasm32` a pointer is
itself a 32-bit offset — so this is about layout, not size.) But **each node still
owns a hashmap with its own allocation**, so this alone doesn't cut allocation count
much. It's the easy, still-mutable version. The real win is going flat.

### Flat (CSR) arena: the version that actually pays off

Store *all* the entries of *all* nodes at a given level in shared, contiguous
arrays, and let each node be just a **range** into the next level's array. For the
depth-2 `E(x,y)` index:

```rust
struct CsrTrie2 {
    xs:  Vec<Value>,   // sorted distinct x values (level 0)
    off: Vec<u32>,     // len xs.len()+1; children of xs[i] are ys[off[i]..off[i+1]]
    ys:  Vec<Value>,   // all y's, grouped by x, sorted within each group (level 1)
}
```

Find the y's for a given x: binary/gallop-search `xs` → index `i` → slice
`ys[off[i]..off[i+1]]`. This is CSR (compressed sparse row) — the standard graph
adjacency layout, which is apt since a triangle query *is* adjacency intersection.

The payoff:
- The whole trie is **~3 `Vec`s = a handful of allocations total**, not
  one-per-node. That's the allocation-churn win (which matters because wasm's
  allocator runs in-band).
- Everything at a level is contiguous → sequential scans, dense cache lines. That's
  the locality win.
- Intersecting a level is now **intersecting two sorted slices (galloping)**
  instead of hash-probing — which is exactly lever #2. So the CSR arena and the
  "sorted-column representation" are the same thing; that's why #1 and #2 overlap.

Depth generalizes: `D` value-arrays + `D` offset-arrays, each offset array linking
level `L` to ranges in level `L+1`. The deepest level's "children" are leaves
(nothing, or a weight if you later do IVM).

### Building it, and the mutation caveat

Build = sort the relation's rows lexicographically by the index's column order, then
one linear scan emits the value-arrays and offset-arrays (a run of equal x's becomes
one `xs` entry plus an `off` boundary). One sort + one pass — replacing
`Trie::build`'s per-row map insertions (`src/join.rs:201`).

Caveat (see also the IVM section): inserting one edge into the middle shifts `ys`
and rewrites `off` → O(n). So CSR is **build-once / batch-rebuild** (rebuild into the
same `Vec`s per delta batch). If you need cheap point updates instead, stay with the
minimal-arena (Vec-of-maps) form or a slab allocator, trading some locality for
mutability.

### How execution changes

Small: `QueryDfsState` holds `Vec<&TrieMap>` today (one current node per atom,
`src/join.rs:506`). With CSR it holds a `Vec<Range<u32>>` — each atom's current
slice. "Descend under key `k`" = find `k` in the slice and replace the range with
`k`'s child range; the proposer is the atom with the shortest range. Same algorithm,
slices instead of hashmaps.
