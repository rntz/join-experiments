// (Claude-generated, reviewed/edited by rntz).
//
// A worked example of the whole query pipeline: build a query, pick a variable order, plan
// it, build indexes, execute, and print the results in terms of the query's own variables.
//
// The query finds triangles of mutual acquaintance:
//
//     knows(x,y), knows(y,z), knows(x,z), x ≤ y, y ≤ z
//
// `knows` is stored symmetrically, so the two ≤ operators keep just one of the six
// orderings of each triangle. (There are no self-loops in our data, so ≤ acts as <.)

use std::rc::Rc;

use rntz_joins::{op, row, Atom, Operator, Query, VecDb};

// Our "database": people, referred to by their index into NAMES, and who knows whom.
// NAMES can be thought of as a pre-built intern table for strings.
const NAMES: [&str; 6] = ["alice", "bob", "carol", "dave", "erin", "frank"];
const FRIENDSHIPS: [(usize, usize); 7] = [
    (0, 1), (1, 2), (0, 2), // alice, bob, carol: a triangle
    (2, 3), (3, 4), (2, 4), // carol, dave, erin: another triangle
    (4, 5),                 // erin & frank: an edge in no triangle
];

fn main() {
    // 0. Put the data in something implementing Database; VecDb is the trivial one. We
    //    store each friendship in both directions.
    let db = VecDb::new().rel("knows", 2,
        FRIENDSHIPS.iter().flat_map(|&(a, b)| [row![a, b], row![b, a]]).collect());

    // 1. Make a query. Variables are `char`s and relations are `&'static str`s.
    let knows = |x, y| Atom { pred: "knows", vars: vec![x, y] };
    let le = |x, y| -> Atom<Rc<dyn Operator>, char> {
        Atom { pred: Rc::new(op::Le), vars: vec![x, y] }
    };
    let query: Query<char, &'static str> = Query {
        vars: vec!['x', 'y', 'z'],
        atoms: vec![knows('x', 'y'), knows('y', 'z'), knows('x', 'z')],
        operators: vec![le('x', 'y'), le('y', 'z')],
    };
    query.self_check(&db); // panics if the query is malformed

    // 2. Pick a variable order.
    let var_order = query.structural_var_order();
    println!("variable order: {var_order:?}");

    // 3. Plan the query execution using the variable order.
    let plan = query.plan(&var_order);

    // 4. Build the trie indexes the plan asks for.
    let indexes = plan.build_indexes(&db);
    println!("need {} indexes: {:?}", indexes.len(), indexes.keys());

    // 5. Bind the indexes into the plan and execute. bind() returns None if some index is
    //    empty, which makes the whole conjunctive query empty.
    let Some(exec) = plan.bind(&indexes) else { return println!("no results") };
    // collect_dfs materializes & sorts the solutions; execute_dfs streams them to a callback.
    let solutions = exec.collect_dfs();

    // Solutions are in variable order, so to print them in terms of query.vars we need
    // where each query variable sits in the variable order.
    let pos: Vec<usize> = query.vars.iter()
        .map(|v| var_order.iter().position(|u| u == v).unwrap())
        .collect();
    println!("{} triangles:", solutions.len());
    for solution in &solutions {
        for (v, &i) in query.vars.iter().zip(&pos) {
            print!("  {v}={}", NAMES[solution[i].untag::<usize>()]);
        }
        println!();
    }
}
