#![allow(unused_variables, dead_code)]

use crate::{Operator, Query, Atom};
use std::collections::HashSet;
use std::collections::HashMap;
use std::hash::Hash;

// Hard rules of variable order picking:
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
//    Non-join vars contribute no useful information for backtracking/pruning (except,
//    again, outputs of operators).

// Hard-enough rules of variable order picking:
//
// 3. Always pick a variable connected by some atom to an already-chosen variable;
//    otherwise we're enumerating a cross-product. If the query is connected, it's always
//    possible to follow this rule. It's conceivable there might be situations where it's
//    best to violate this rule: e.g. if the disconnected var has a small domain, and
//    binding it allows some very selective operator to fire, maybe it could be worth it?
//    My intuition is that this is rare, if possible. TODO: try to come up with such an
//    example.

// Heuristics for variable order picking:
//
// XXX TODO XXX

impl<Var, Rel, Op> Query<Var, Rel, Op>
where Var: Clone+Hash+Eq+Ord, Rel:Clone+Hash+Eq, Op: Operator
{
    fn structural_var_order(self: &Query<Var, Rel, Op>) -> Vec<Var> {
        // Pick using only the hard rules above, plus the heuristic:
        //
        // - pick vars more strongly connected to already chosen vars.
        //
        // how strong is a connection? let's say: count the number of co-occurrences; a
        // co-occurrence is an (atom,v,v') where v is the candidate variable, {v,v'} ⊆
        // atom.vars, and v' is already in the chosen var order prefix.
        todo!("pick var order using only structural features and no statistics");
    }
}

// =====  VARIABLE ORDER PICKING via CARDINALITY ESTIMATION =====
//
// The cost-based way to pick variables or plan queries:
//
// Have some way to either estimate or upper-bound the cost of executing a (prefix of a)
// variable order (a "cost model"). To first order, the cost of a var order is the sum
// over its prefixes of the # of solutions satisfying that prefix. Crucial bit is
// estimating # of solutions satisfying a prefix (a "cardinality estimator").
//
// (A slightly better cost model takes into account the width of each level; more atoms =
// more work, because we have to filter by each atom. But the most important factor by far
// is # of bindings. Sometimes, confusingly, "cost model" is used to refer exclusively to
// estimating the cost GIVEN a cardinality estimate, i.e. the per-tuple cost.)
//
// Then, search for a variable order that minimizes this cost. You're trying to minimize
// the cost for the full query, but you use the cost so far as a heuristic when searching.
// The obvious greedy algorithm repeatedly picks the variable that minimizes the cost of
// the prefix so far, until done. Better: a beam search. There are fancier approaches
// (dynamic programming?) but I don't think we'll need them.

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

// ===== OPTIMISTIC VS PESSIMISTIC ESTIMATORS =====
//
// There are ~two kinds of cardinality estimator: those that aim for the average
// ("optimistic"); those that give a hard upper bound ("pessimistic"); or something in
// between. Using "pessimistic" upper bounds is more robust against adversarial data /
// less likely to pick a plan with a bad worst case, because it tries to pick the order
// with the lowest upper bound / the best worst case. However, per the name, worst-case
// optimal joins already have fairly good robustness, and a more optimistic estimate often
// makes more efficient use of limited statistics to find a good variable order. So I lean
// towards an optimistic estimator, even though these can be quite inaccurate (famous
// paper: "How Good are Query Optimizers, Really?").
//
// TODO: explain typical "independence" assumption of cost estimators.
