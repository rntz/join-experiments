// (Claude-generated.)
//
// Tests for Query::ground_vars and Query::self_check. As in queries.rs, variables are
// `char` and relations are &str. self_check panics on a malformed query, so each check
// gets a #[should_panic] test.
use std::rc::Rc;

use rntz_joins::{Atom, Operator, Query, VecDb};
use rntz_joins::op;

fn db() -> VecDb {
    VecDb::new()
        .rel("E", 2, vec![vec![0, 1], vec![1, 2]])
        .rel("T", 1, vec![vec![5]])
}

fn atom(rel: &'static str, vars: &[char]) -> Atom<&'static str, char> {
    Atom { pred: rel, vars: vars.to_vec() }
}

fn op_atom(o: impl Operator + 'static, vars: &[char]) -> Atom<Rc<dyn Operator>, char> {
    Atom { pred: Rc::new(o), vars: vars.to_vec() }
}

#[test]
fn test_ground_vars() {
    // E(x,y), x + y = z: the atom grounds x,y and the operator then grounds z.
    let q: Query<char, &'static str> = Query {
        vars: vec!['x', 'y', 'z'],
        atoms: vec![atom("E", &['x', 'y'])],
        operators: vec![op_atom(op::Add, &['x', 'y', 'z'])],
    };
    assert_eq!(q.ground_vars(), vec!['x', 'y', 'z']);
    q.self_check(&db());

    // Saturation across operators listed in the "wrong" order: z + z = w needs z, which
    // x + y = z only supplies on a later pass.
    let q: Query<char, &'static str> = Query {
        vars: vec!['x', 'y', 'z', 'w'],
        atoms: vec![atom("E", &['x', 'y'])],
        operators: vec![op_atom(op::Add, &['z', 'z', 'w']), op_atom(op::Add, &['x', 'y', 'z'])],
    };
    assert_eq!(q.ground_vars(), vec!['x', 'y', 'z', 'w']);
    q.self_check(&db());

    // An operator without an output grounds nothing, so y stays ungrounded.
    let q: Query<char, &'static str> = Query {
        vars: vec!['x', 'y'],
        atoms: vec![atom("T", &['x'])],
        operators: vec![op_atom(op::Le, &['x', 'y'])],
    };
    assert_eq!(q.ground_vars(), vec!['x']);
}

#[test] #[should_panic(expected = "query variable 'z' is not grounded")]
fn test_ungrounded_var() {
    let q: Query<char, &'static str> = Query {
        vars: vec!['x', 'y', 'z'], atoms: vec![atom("E", &['x', 'y'])], operators: vec![],
    };
    q.self_check(&db());
}

#[test] #[should_panic(expected = "duplicate query variable 'x'")]
fn test_duplicate_var() {
    let q: Query<char, &'static str> = Query {
        vars: vec!['x', 'y', 'x'], atoms: vec![atom("E", &['x', 'y'])], operators: vec![],
    };
    q.self_check(&db());
}

#[test] #[should_panic(expected = r#"atom "E"('x') has the wrong number of variables"#)]
fn test_atom_arity() {
    let q: Query<char, &'static str> = Query {
        vars: vec!['x'], atoms: vec![atom("E", &['x'])], operators: vec![],
    };
    q.self_check(&db());
}

#[test] #[should_panic(expected = "operator Le('x') has the wrong number of variables")]
fn test_operator_arity() {
    let q: Query<char, &'static str> = Query {
        vars: vec!['x', 'y'],
        atoms: vec![atom("E", &['x', 'y'])],
        operators: vec![op_atom(op::Le, &['x'])],
    };
    q.self_check(&db());
}

#[test] #[should_panic(expected = r#"atom "E"('x', 'y') uses 'y', which is not in query.vars"#)]
fn test_undeclared_var() {
    let q: Query<char, &'static str> = Query {
        vars: vec!['x'], atoms: vec![atom("E", &['x', 'y'])], operators: vec![],
    };
    q.self_check(&db());
}
