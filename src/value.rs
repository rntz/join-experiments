// I'm assuming we intern everything up front. This makes things simpler than figuring out
// where to put tags to minimize tag-checking overhead.
pub type Value = usize;         // needs to be Copy + Hash + Eq

// TODO: rewrite join.rs etc so that they only need Clone + Hash + Eq instead, which would
// mean we could slot in a tagged representation:

// #[derive(Clone, Hash, PartialEq, Eq)]
// pub enum Value { Int(usize), Str(Rc<String>) }
// impl From<usize> for Value {
//     fn from(x: usize) -> Value { Value::Int(x) }
// }

// impl TryFrom<Value> for usize {
//     fn try_from(x: Value) {
//         match x {
//             Value::Int(n) => Some(n),
//             _ => todo!()
//         }
//     }
// }
