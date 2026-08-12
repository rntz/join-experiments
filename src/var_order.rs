use crate::{Operator, Query, Atom};
use std::cmp::Reverse;
use std::collections::HashSet;
use std::collections::HashMap;
use std::hash::Hash;

// Hard rules of variable order picking:
//
// 0. Only pick grounded variables, otherwise we can't execute the query. A relational
//    atom grounds all its vars; an operator grounds its output once its inputs are
//    ground. (See Query::ground_vars().)
//
// 1. Immediately pick determined variables, for two reasons:
//
//    a) They are "free"; because uniquely determined, they require no branching.
//
//    b) Operators are currently only consulted when their last var is bound. Delaying
//    this variable can therefore delay an operator, even when it could cause useful
//    failure/backtracking - this would be bad.
//
// 2. Always pick join vars before singleton vars (except for rule 1).
//
//    Non-join vars contribute no useful information for backtracking/pruning (except for
//    operator outputs).
//
// Heuristics for variable order picking:
//
// 3. When possible, pick a variable connected by some atom to an already-chosen variable;
//    otherwise we're enumerating a cross-product. For purely relational (operator-free)
//    queries, this is always possible if the query is connected. See "QUERY CONNECTEDNESS
//    and its MALCONTENTS".

// ===== QUERY CONNECTEDNESS and its MALCONTENTS =====
//
// We want to ensure that we can pick a variable order while (a) only picking
// variables which are grounded given the previously chosen vars, and which (b) either
//
// - share an relational atom with an already-chosen var, or
// - are the output of an operator atom all of whose inputs are chosen.
//
// Ideally, we could also (c) start from any grounded variable and not have to
// backtrack.
//
// (a) is necessary for the var order to be executable. (b) makes execution more
// efficient - it avoids enumerating cross products. (c) makes the var order picker's
// job easier.
//
// In an fully relational, operator-free query, (a) always holds, and (b-c) hold iff
// the query hypergraph is connected. With operators, it's more complicated. E.g. if
// our query is
//
//     Q1(x,y,z) = R(x,y), x + y = z
//
// Then our variable order must start with x or y. This still satisfies our criteria.
// But consider:
//
//     Q2(x,y,z) = R(x,y), x + y = z, S(z)
//
// Now z is grounded by S, so we could start with z. But if we start with z, we must
// violate (b): x,y are not connected to z by any relational atom, and we can't fire
// the operator (x + y = z) knowing only z. So we must either ban Q2, or give up
// either (b) or (c). Banning Q2 but not Q1 is weird, though: the presence of S(z)
// makes Q2 *easier* to execute than Q1, so why ban Q2 and not Q1?
//
// For an even more pathological example:
//
//     Q3(x,y,z) = R(x), S(y), x + y = z
//
// There is no way to run this query without a cross product of R & S. So we must
// either ban this query (even though it's quite reasonable for small R/S) or give up
// on (b).
//
// After discussion with Kris, we think the right answer is to give up on (b), but
// have a variable order picker that tries to avoid cross products if possible.


// ===== STRUCTURAL vs STATISTICAL PLANNING =====
//
// We can pick a variable order looking only at the query ("structural") or also looking
// at some summary statistics for the database ("statistical"). I've implemented a
// structural picker to start with; I'll get to the statistical one later. Here's the
// structural one:

impl<Var, Rel, Op> Query<Var, Rel, Op>
where Var: Clone+Hash+Eq, Rel:Clone+Hash+Eq, Op: Operator {
    // NB. O(n^2) in the # of variables.
    //
    // Pick using only the hard rules above, plus the heuristic: pick vars more strongly
    // connected to already chosen vars. How strong is a connection? We count the number
    // of co-occurrences (atom,v,v') where v is the candidate variable, v' is already
    // chosen, and {v,v'} ⊆ atom.vars.
    //
    // Operators count as atoms throughout: sharing one is a weaker connection than sharing
    // a relational atom, but binding v still brings the operator closer to firing.
    pub fn structural_var_order(self: &Query<Var, Rel, Op>) -> Vec<Var> {
        // Relational atoms and operators, uniformly, as lists of vars.
        let atoms: Vec<&[Var]> = self.atoms.iter().map(|a| &a.vars[..])
            .chain(self.operators.iter().map(|a| &a.vars[..]))
            .collect();
        // var_atoms[v] = positions in `atoms` of the atoms mentioning v.
        let mut var_atoms: HashMap<Var, Vec<usize>> = HashMap::new();
        for (i, &vars) in atoms.iter().enumerate() {
            for v in vars {
                let v_atoms = var_atoms.entry(v.clone()).or_default();
                // Avoid pushing the same atom twice if it uses a var twice.
                if v_atoms.last() != Some(&i) { v_atoms.push(i) }
            }
        }
        let var_atoms = var_atoms;
        let degree = |v: &Var| var_atoms[v].len();

        let mut order: Vec<Var> = Vec::with_capacity(self.vars.len());
        let mut chosen: HashSet<Var> = HashSet::new();
        // Not-yet-fired operators with outputs.
        let mut unfired: Vec<&Atom<Op, Var>> = self.operators.iter()
            .filter(|atom| atom.pred.has_output())
            .collect();
        // Rule 0: We only consider vars grounded by relational atoms; outputs of
        // operators get chosen by firing the operators once their inputs are chosen. We
        // preserve the order of vars b/c we use that order as a tiebreak to ensure
        // determinism.
        let candidates: Vec<Var> = self.vars.iter()
            .filter(|v| self.atoms.iter().any(|a| a.vars.contains(v)))
            .cloned()
            .collect();

        loop {
            // Rule 1: Fire any operators whose inputs are all chosen. This ensures we put
            // determined variables first.
            unfired.retain(|atom| {
                let (inputs, outputs) = atom.vars.split_at(atom.pred.input_arity());
                assert!(outputs.len() == 1);
                let output = &outputs[0];
                if chosen.contains(output) { return false; }
                if inputs.iter().all(|v| chosen.contains(v)) {
                    order.push(output.clone());
                    chosen.insert(output.clone());
                    return false;
                }
                return true;
            });

            if order.len() == self.vars.len() { break }

            // Pick a var according to rules 2 & 3 & heuristics.
            let connectedness = |v: &Var| -> usize {
                var_atoms[v].iter()
                    .map(|&i| atoms[i].iter().filter(|u| chosen.contains(*u)).count())
                    .sum()
            };
            let next: &Var = candidates.iter()
                .filter(|v| !chosen.contains(v))
                .min_by_key(|v| (Reverse(degree(v) > 1),    // rule 2
                                 Reverse(connectedness(v)), // connectedness heuristic / rule 3
                                 Reverse(degree(v))))       // most constrained var
                .expect("no atom can bind any remaining var; is the query grounded? \
                         see Query::self_check");
            chosen.insert(next.clone());
            order.push(next.clone());
        }
        order
    }
}


// ===== STATISTICAL VARIABLE ORDER PICKING via CARDINALITY ESTIMATION =====
//
// The cost-based way to pick variables or plan queries:
//
// Have some way to either estimate or upper-bound the cost of executing a (prefix of a)
// variable order (a "cost model"). To first order, the cost of a var order is the sum
// over its prefixes of the # of solutions satisfying that prefix. Crucial bit is
// estimating # of solutions satisfying a prefix (a "cardinality estimator"). [1]
//
// Then, search for a variable order that minimizes this cost. You're trying to minimize
// the cost for the full query, but you use the cost so far as a heuristic when searching.
// The obvious greedy algorithm repeatedly picks the variable that minimizes the cost of
// the prefix so far, until done. Better: a beam search. There are fancier approaches
// (dynamic programming?) but I don't think we'll need them.
//
// [1] In the literature, confusingly, sometimes "cost model" means only estimating the
// cost of a query GIVEN the cardinality estimate, i.e. per-tuple cost.

// ===== OPTIMISTIC VS PESSIMISTIC ESTIMATORS =====
//
// There are ~two kinds of cardinality estimator: those that aim for the average
// ("optimistic"); those that give a hard upper bound ("pessimistic"); or something in
// between. Using "pessimistic" upper bounds is more robust against adversarial data /
// less likely to pick a plan with a bad worst case, because it tries to pick the order
// with the lowest upper bound / the best worst case. However, per the name, worst-case
// optimal joins already have fairly good robustness, and a more optimistic estimate often
// makes more efficient use of limited statistics to find a good variable order. So I lean
// towards an optimistic estimator, even though these can be quite inaccurate.
//
// A crucial assumption of typical optimistic cardinality evaluators is independence of
// join criteria: if two criteria constrain the same variable, assume the probability of
// passing each is independent of the other. This is "optimistic" because often criteria
// are correlated, which results in more than the predicted # of results.
//
// Relatively readable papers which touch this subject:
//
// # "How Good are Query Optimizers, Really", https://www.vldb.org/pvldb/vol9/p204-leis.pdf
//
// Shows that typical cardinality estimators can strongly under-estimate query result
// sizes, although this has less impact than you might fear. Section 2.3 summarizes the
// textbook approach to cardinality estimation used by PostgreSQL.
//
// # "Pessimistic Cardinality Estimation", https://arxiv.org/pdf/2412.00642
//
// A survey paper on pessimistic, bounds-based cardinality estimation. BoundSketch
// (discussed in section 7, "The Chain Bound") seems like it might be a reasonable
// approach for our situation.

#[allow(dead_code, unused_variables)]
mod unfinished_estimator {
    use crate::{Operator, Query, Atom};
    use std::collections::HashSet;
    use std::collections::HashMap;
    use std::hash::Hash;

    #[derive(Clone)]
    struct Candidate<Var> {
        order: Vec<Var>,      // vars picked, in order
        todo: HashSet<Var>,   // vars not yet picked
        determined: Vec<Var>, // vars uniquely determined but not yet picked
        cost: f64,            // cost (cardinality) estimate
    }

    impl<Var: Eq + Hash + Clone> Candidate<Var> {
        fn new<Rel: Eq + Hash + Clone, Op: Operator>(query: &Query<Var, Rel, Op>) -> Candidate<Var> {
            Candidate {
                order: vec![],
                todo: query.vars.iter().cloned().collect(),
                cost: 0.0,
                determined: query.operators.iter()
                    .filter(|atom| atom.pred.has_output() && atom.pred.input_arity() == 0)
                    .map(|atom| atom.vars[0].clone())
                    .collect(),
            }
        }

        fn children(mut self) -> Vec<Var> {
            if let Some(x) = self.determined.pop() {
                return vec![x]
            };
            todo!()
        }
    }

    fn beam_search<Var: Eq + Hash + Clone, Rel: Eq + Hash + Clone, Op: Operator>(
        query: &Query<Var, Rel, Op>,
        beam_size: usize,
    ) -> Result<Vec<Var>, String> {
        assert!(beam_size > 0);
        // Atoms & operators that cover a given var. Useful for finding connected vars.
        let mut var_atoms: HashMap<&Var, Vec<&Atom<Rel,Var>>> = HashMap::new();
        let mut var_opers: HashMap<&Var, Vec<&Atom<Op,Var>>> = HashMap::new();
        for atom in &query.atoms {
            for var in &atom.vars {
                var_atoms.entry(var).or_default().push(atom);
            }
        }
        for atom in &query.operators {
            for var in &atom.vars {
                var_opers.entry(var).or_default().push(atom);
            }
        }

        // Picking.
        let mut prefixes: Vec<Candidate<Var>> = Vec::with_capacity(beam_size);
        let mut children: Vec<Candidate<Var>> = Vec::with_capacity(beam_size);
        prefixes.push(Candidate::new(query));
        for _level in 0..query.vars.len() {
            assert!(!prefixes.is_empty());
            assert!(prefixes.len() <= beam_size);
            assert!(prefixes.is_sorted_by_key(|x| x.cost));
            for candidate in &prefixes {
                todo!("find children, add them to `children` if they're good enough")
            }
            std::mem::swap(&mut prefixes, &mut children);
            children.clear();
        }
        assert!(!prefixes.is_empty());
        assert!(prefixes.len() <= beam_size);
        assert!(prefixes.is_sorted_by_key(|x| x.cost));
        assert!(prefixes[0].order.len() == query.vars.len());
        assert!(prefixes[0].todo.is_empty());
        prefixes.drain(..)
            .map(|c| c.order)
            .next()
            .ok_or("should have found at least one order".into())
    }
}
