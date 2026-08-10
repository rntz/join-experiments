use std::fmt::Debug;

use crate::Value;

// ---------- OPERATORS, or ATOMS THAT COMPUTE ----------
//
// I'm going to explain the current Operator design by first showing examples of the full
// space of operators I'd like to eventually cover, then the restrictions on this space
// I've imposed to simplify the implementation.

// ===== SPACE OF OPERATORS BY EXAMPLE =====
//
//              SYNTAX          INPUTS      OUTPUTS     DO INPUTS UNIQUELY DETERMINE OUTPUT?
// INEQUALITY   x ≤ y           x,y         none        yes
//
// CONSTANT     x = 2           none        x           yes
//
// EQUALITY     x = y           x           y           yes, ∀x ∃!x  x=y
//                              y           x           yes, ∀y ∃!x  x=y
//
// RANGE        i ∈ range(n,m)  n,m         i           no
//
// ADDITION     x + y = z       x,y         z           yes; ∀x,y ∃!z  x + y = z
//                              x,z         y           yes; at most one y for fixed (x,z)
//                              y,z         x           yes; at most one x for fixed (y,z)
//
// STRING       x ++ y = z      x,y         z           yes; ∀x,y ∃!z
// APPEND                       z           x,y         no; many x,y yield same z
//                              x,z         y           yes; ∀x,z ∃ at most one y (maybe none)
//                              y,z         x           yes; ∀y,z ∃ at most one x (maybe none)
//
// Addition & string append are good examples of operators with multiple possible
// input-output modes ("modes" is the semi-standard term for this in logic programming).
// Given strings x,y we can compute z = x ++ y. But given z, we can ask for all x ++ y =
// z, all splittings of it. And given y,x we can ask: is x a prefix of z, and if so,
// what's the suffix?

// ===== WHAT WE ACTUALLY IMPLEMENT =====
//
// We require operators to act like partial functions, and have: (0) a fixed input/output
// direction; (1) at most 1 output variable; and (2) the inputs must uniquely determine
// the outputs. To drop these restrictions we'd need to redesign the Operator trait,
// modify the variable order picker, and perhaps also change QueryPlan etc.
//
// Further comments on each limitation:
//
// 0. Fixed input/outputs limit our choice of variable order / direction of information
//    flow. Eg. if OpEq is an equality Operator, then (x = y) can become:
//
//      Atom { pred: OpEq, vars: [x,y] }        which makes x input and y output
//      Atom { pred: OpEq, vars: [y,x] }        which makes y input and x output
//
//    But we can't have one atom that represents both, so we can't leave it up to the
//    planner to decide which way information flows.
//
// 1. Since an operator has at most one output variable, the query planner/executor
//    consults an operator only on the level of its last variable. With multiple output
//    variables, we'd need to consult the operator for each output variable after the
//    inputs are bound, and possibly also cache the output(s).
//
// 2. Because inputs functionally determine the output, i.e. there is at most one output
//    per input tuple, the variable order picker always emits output vars immediately once
//    their inputs become available.
//
//    Note also: to backtrack as soon as possible and not waste work, we should consult an
//    operator as soon as it can *fail* -- in principle, as soon as all its inputs are
//    bound. But we currently only consult it when its last variable is bound -- this
//    might be its output, not its last input! But this is okay because the var order
//    picker is guaranteed to emit the output var immediately after all inputs are bound,
//    so we're not really delaying.

// Debug is a supertrait so that error messages can name the operator; it also gives us
// `dyn Operator: Debug`, and hence Debug for the default `Rc<dyn Operator>` queries.
pub trait Operator: Debug {
    // inputy_arity = number of input variables
    // arity        = number of input + output variables
    fn input_arity(&self) -> usize;
    fn arity(&self) -> usize { self.input_arity() + (self.has_output() as usize) }
    fn has_output(&self) -> bool; // at most one output, for

    // There are basically 2 kinds of operators:
    // - "check":   operators without output
    // - "compute": operators with output
    //
    // check operators should implement only check().
    // compute operators should implement both check() and compute().
    //
    // check(args) accepts arity() arguments and returns true if they satisfy the
    // operator. compute(inputs) accepts input_arity(args) and returns Some(output) on
    // success, None on failure.

    // Precondition: args.len() == self.arity().
    fn check(&self, args: &[Value]) -> bool;
    // Precondition: self.has_output() && inputs.len() == self.input_arity().
    fn compute(&self, inputs: &[Value]) -> Option<Value>;
}

// The Operator trait lets us choose how to dispatch on operators. If we have a concrete
// `enum MyOps { ... }` for all the query operators we need, we can implement `Operator
// MyOps` by matching on this enum. On the other hand, using `Rc<dyn Operator>` makes it
// easy to use any Operator without a big enum/match, but requires dynamic dispatch
// through function pointers at runtime, which may be slower (esp. on WebAssembly).
//
// We use Rc instead of Box because Query::plan clones operators; Rc is cheap to clone.
impl<Ptr: Debug + std::ops::Deref<Target = dyn Operator>> Operator for Ptr {
    fn input_arity(&self) -> usize { (**self).input_arity() }
    fn has_output(&self) -> bool { (**self).has_output() }
    fn arity(&self) -> usize { (**self).arity() }
    fn check(&self, args: &[Value]) -> bool {
        debug_assert!(args.len() == (**self).arity());
        (**self).check(args)
    }
    fn compute(&self, inputs: &[Value]) -> Option<Value> {
        debug_assert!(inputs.len() == (**self).input_arity());
        (**self).compute(inputs)
    }
}

// If your query has no operators, you could also use Empty.
#[allow(dead_code)]
#[derive(Clone, Debug)]
enum Empty {}                   // useful representation if your query has no operators.
impl Operator for Empty {
    #[inline] fn input_arity(&self) -> usize { match *self {} }
    #[inline] fn has_output(&self) -> bool { match *self {} }
    #[inline] fn check(&self, _: &[Value]) -> bool { match *self {} }
    #[inline] fn compute(&self, _: &[Value]) -> Option<Value> { match *self {} }
}

// ==== ON IMPLEMENTING OPERATORS EFFICIENTLY ====
//
// It's possible dispatch overhead for operators will become a bottleneck for query
// execution in some cases. If so, it's worth investigating Frank McSherry's approach to
// them, which attempts to dispatch as infrequently as possible by having operators
// process large "chunks" of data at a time.
//
// He does this via a breadth-first approach to solving WCOJs, maintaining a vector of
// partial solutions for the first N variables, then extending to partial solutions for
// N+1, etc. To see an example of this, look at the examples/join-v1.rs prototype. See
// also his blog post:
//
// https://github.com/frankmcsherry/blog/blob/master/posts/2025-12-23.md
//
// Email me (Michael Arntzenius, daekharel@gmail.com) if you're having trouble
// understanding it or how it relates to this implementation; or email Frank and cc me,
// he's quite friendly (but won't know anything about this implementation).
//
// Note that it's a little more complicated than just switching depth for breadth. The
// Claude-generated bfs in join_bfs.rs, for instance, is not Frank-like and would not
// improve performance. Also, breadth-first search can be very memory hungry if there are
// lots of results; if this is a problem it might be worth doing something in-between BFS
// & DFS involving fixed-length chunks of partial solutions.
//
// Our current approach is depth-first and more like a hash-based version of LFTJ. You can
// read the LFTJ paper if you like, but this code is probably easier to understand.
//
// LFTJ paper: https://arxiv.org/abs/1210.0481
//
// LFTJ uses a "trie iterator" interface. If you line things up right it's possible for
// many computational atoms to satisfy this interface. You can think of this as
// materializing the trie lazily/on-demand. Of course, computational atoms can't
// materialize levels of the trie that correspond to their *input* variables, but they
// can be told to "seek to position x" (this assigns that input variable to x). As long
// as *some* atom/trie iterator can materialize a list of candidates for this variable,
// things work out eventually.
//
// See section 3.4, p6, list item 1, which discusses equality atoms, and section 6.2,
// numbered list, elements 2-3 ("Functions", "Primitives") and 6 ("Ranges"). (Note that
// "Function" does NOT mean computational function here: it means functional dependency.)


// ---------- OPERATOR IMPLEMENTATIONS ----------

// Addition x + y = z. Inputs x, y; output z. Panics on overflow; this might slow it down.
#[derive(Clone, Copy, Debug)]
pub struct Add;
impl Operator for Add {
    fn input_arity(&self) -> usize { 2 }
    fn has_output(&self) -> bool { true }
    fn check(&self, args: &[Value]) -> bool {
        let &[x, y, z] = args else { panic!("Add::check wants 3 args") };
        x.checked_add(y).expect("addition overflow") == z
    }
    fn compute(&self, inputs: &[Value]) -> Option<Value> {
        let &[x, y] = inputs else { panic!("Add::compute wants 2 inputs") };
        Some(x.checked_add(y).expect("addition overflow"))
    }
}

// Inequality x ≤ y. Inputs x, y; no output, so it only ever checks.
#[derive(Clone, Copy, Debug)]
pub struct Le;
impl Operator for Le {
    fn input_arity(&self) -> usize { 2 }
    fn has_output(&self) -> bool { false }
    fn check(&self, args: &[Value]) -> bool {
        let &[x, y] = args else { panic!("Le::check wants 2 args") };
        x <= y
    }
    fn compute(&self, _: &[Value]) -> Option<Value> {
        panic!("Le has no output; do not call compute()")
    }
}
