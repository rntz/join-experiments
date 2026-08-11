// (Claude-generated.)
//
// Tests for Query::structural_var_order. As in queries.rs, variables are `char` and
// relations are &str. Each picked order is fed to plan(), which panics if the order isn't
// executable (some variable with no proposer).
use std::rc::Rc;

use rntz_joins::{Atom, Operator, Query, Value};
use rntz_joins::op;

fn atom(rel: &'static str, vars: &[char]) -> Atom<&'static str, char> {
    Atom { pred: rel, vars: vars.to_vec() }
}

fn op_atom(o: impl Operator + 'static, vars: &[char]) -> Atom<Rc<dyn Operator>, char> {
    Atom { pred: Rc::new(o), vars: vars.to_vec() }
}

fn order(
    vars: &[char],
    atoms: Vec<Atom<&'static str, char>>,
    operators: Vec<Atom<Rc<dyn Operator>, char>>,
) -> Vec<char> {
    let q: Query<char, &'static str> = Query { vars: vars.to_vec(), atoms, operators };
    let order = q.structural_var_order();
    q.plan(&order);
    order
}

// x = 2: no inputs, so its output is determined from the start.
#[derive(Debug)]
struct Const(Value);
impl Operator for Const {
    fn input_arity(&self) -> usize { 0 }
    fn has_output(&self) -> bool { true }
    fn check(&self, args: &[Value]) -> bool { args[0] == self.0 }
    fn compute(&self, _: &[Value]) -> Option<Value> { Some(self.0.clone()) }
}

// Every var of a triangle is a join var of the same degree, so ties send us through the
// vars in declaration order.
#[test]
fn test_triangle() {
    let atoms = vec![atom("E", &['x', 'y']), atom("E", &['y', 'z']), atom("E", &['z', 'x'])];
    assert_eq!(order(&['x', 'y', 'z'], atoms, vec![]), vec!['x', 'y', 'z']);
}

// Rule 2: join vars first, singletons last.
#[test]
fn test_singletons_last() {
    // R(x,y) S(y,z): y is the only join var.
    let atoms = vec![atom("R", &['x', 'y']), atom("S", &['y', 'z'])];
    assert_eq!(order(&['x', 'y', 'z'], atoms, vec![]), vec!['y', 'x', 'z']);

    // R(x,w) S(y,w) T(z,w)
    // A star around w, which every atom mentions.
    let atoms = vec![atom("R", &['x', 'w']), atom("S", &['y', 'w']), atom("T", &['z', 'w'])];
    assert_eq!(order(&['x', 'y', 'z', 'w'], atoms, vec![]), vec!['w', 'x', 'y', 'z']);
}

// Rule 1: determined vars are picked the moment their inputs are.
#[test]
fn test_determined_immediately() {
    // s = x + y comes as soon as x,y are chosen, ahead of the unrelated w.
    let atoms = vec![atom("T", &['w']), atom("E", &['x', 'y'])];
    let ops = vec![op_atom(op::Add, &['x', 'y', 's'])];
    assert_eq!(order(&['w', 'x', 'y', 's'], atoms, ops), vec!['x', 'y', 's', 'w']);

    // A constant is determined before anything else.
    let atoms = vec![atom("E", &['x', 'y'])];
    assert_eq!(order(&['x', 'y'], atoms, vec![op_atom(Const(2.into()), &['y'])]), vec!['y', 'x']);
}

// Sharing an operator counts as a connection: x,z are join vars, so we bind them (and run
// the filter) before enumerating y,w.
#[test]
fn test_operator_connects() {
    // Rxy Szw x≤z, or:
    // y --[R]-- x --[≤]-- z --[S]-- w
    let atoms = vec![atom("R", &['x', 'y']), atom("S", &['z', 'w'])];
    let ops = vec![op_atom(op::Le, &['x', 'z'])];
    assert_eq!(order(&['x', 'y', 'z', 'w'], atoms, ops), vec!['x', 'z', 'y', 'w']);
}
