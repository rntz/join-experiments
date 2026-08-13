- triangles.rs: Why is OPERATOR-FILTERED so slow on cit-HepTh?
  is this a regression?

- triangles.rs: why is tagged slower than untagged on cit-HepTh on OPERATOR-FILTERED? is
  this expected? seems expected: operators need to check tags now. write it up.

# THINGS TO MAYBE DO BEFORE LEAVING.

- move big comment from join.rs into README.md
- WRITE UP THOUGHTS ON MUTATION into MUTATION.md
- write up a todo list into README.md

- FDs in the schema, use them in var order picker
- Query::run() that undoes the var order in final results
- constants in atoms
- test the variable order picker end-to-end in query execution
- quarantine the half-finished beam search code.

# WHAT SHOULD I WRITE UP BEFORE LEAVING

- HOW TO HANDLE MUTATION:
  diffs & delta queries
  reuse query infrastructure to run delta queries
  maintain indexes over updates
  link to "Fixing Incremental Computation", cite the correct figure.

- explain WCOJs and asymptotics.

# High level goals not started on yet

- aggregations
- tensors??
- mutation & incremental maintenance over it

# Pieces of joins I haven't implemented yet

- interning!
- constants in queries!
- picking a variable order!

# Nice to haves I haven't implemented yet

- extending along FDs
- semijoins

# TODOs

- tagged value alternative in src/value.rs.

- FDs in the schema: figure out how to represent functional dependency info in the
  Database/Schema trait (e.g. per-relation primary keys), for extending along FDs during
  planning/var order picking.

- constants in atoms: decide how to represent constant arguments in Atom (e.g.
  R(x,2)). Trie::build already supports EqConst shapes; this is about the
  query-level representation.

# TODOs for later

- implement 4-clique (K4) benchmark, should show a more significant speedup than
  triangles compared with non-WCO join. Claude suggests comparing against a
  2-step binary join plan: find triangles, then extend to 4-cliques.

- maybe: debug perf of execute_dfs() using callgrind?
  would need to run it on an x86_64 box instead of Mac ARM.
