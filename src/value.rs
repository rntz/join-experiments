// ========== REPRESENTING VALUES ==========
//
// There are at least three reasonable representation choices:
//
// 1. TAG EVERYTHING -- Tag every value; pay tagging overhead in space and time.
// 2. INTERNING -- Everything is usize; intern anything that doesn't fit in 64 bits.
// 3. SMART TAGGING -- Tag intelligently, e.g. per-column.
//
// (1) is simplest but probably slowest.
//
// (2) requires carefully managing interning tables. TODO: explain the subtlety of needing
// a second intern table for temporary data during operations.
//
// (3) requires a careful, thoughtful redesign of the algorithmic core (the Trie index
// data structure & join implementation).
//
// My advice: implement both (1) and (2) and compare their performance. If the performance
// of (1) is acceptable, use it, because it keeps the code simple and the data
// self-describing rather than requiring interning tables everywhere.
//
// I've gone with (2) for now but have yet to write the interning/deinterning code.


// ==================== STRUCT of USIZE (strategy 2) ====================
//
// Values are represented by a simple struct wrapping usize:
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)] // TODO: stop deriving Copy!
pub struct Value(usize);

// The purpose of the struct is future-proofing: it requires explicitly converting into
// and out of Value. This will make it easier to switch to a tagged representation later
// if desired.

use std::convert::Infallible;
use std::fmt::Debug;

impl Debug for Value {
    fn fmt(&self, m: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        self.0.fmt(m)
    }
}

impl From<usize> for Value { fn from(x: usize) -> Value { Value(x) } }

// A type that can be converted to Value by tagging, and out of Value by tag checking.
pub trait ValueType: Sized {
    fn to_value(self) -> Value;
    fn from_value(value: Value) -> Result<Self, TagError>;
}
pub type TagError = Infallible; // no tag errors exist in this representation.

impl ValueType for usize {
    fn to_value(self) -> Value { Value(self) }
    fn from_value(value: Value) -> Result<usize, TagError> { Ok(value.0) }
}

// Convenience method for tag-checking & unwrapping.
impl Value {
    pub fn untag<X: ValueType>(self) -> X { let Ok(v) = X::from_value(self); v }
}


// // ===============================================================================
// // ============================ UNTAGGED USIZE VALUES ============================
// // ===============================================================================

// use std::convert::Infallible;

// // I'm assuming we intern everything up front. This makes things simpler than figuring out
// // where to put tags to minimize tag-checking overhead.
// pub type Value = usize;         // needs to be Copy + Hash + Eq

// // It would be nice to be able to easily try out a tagged-value implementation to compare
// // performance. How can we do this? Three ways:
// //
// // 1. Redefine Value to be an enum and rewrite the code that touches Value.
// //
// //    I've put such an enum & support code in the second, commented-out half of this file.
// //    But you'd have to rewrite join.rs, op.rs etc; annoying to do by hand. Throw an LLM
// //    at it & it should do fine.
// //
// //    This is fine for a one-time experiment, and I'd recommend doing this experiment as
// //    soon as you have some realistic and large-enough test cases to check out the
// //    performance difference.
// //
// // 2. Make our code agnostic to the Value representation so we can comment out "pub type
// //    Value = usize" and uncomment the second half of this file.
// //
// //    I've done this halfway in src/op.rs via untag() & into().
// //
// //    The problem with this is that it's not checked by the compiler so it requires
// //    constant vigilance when writing new code. Unrealistic; in fact things will bitrot
// //    and you'll get compiler errors when switching representations; degrades to (1).
// //
// // 3. Parameterize code over the Value type. The compiler-checked version of (2). This is
// //    annoying because it adds yet more type parameters to Database, Query, QueryPlan,
// //    Operator, etc.
// //
// //    If you want to do more than a one-time experiment with tagged vs untagged values, I
// //    recommend this. Figure out where the type parameters should go (can any of them
// //    become associated types on traits, eg), then fix the resulting compiler errors (or
// //    throw an LLM at them).
// //
// // I wish Rust had an ML-like (SML, OCaml) module system. That would make (3) much nicer.

// // ---------- GARBAGE CODE ----------
// //
// // This code only exists so that we can easily comment this part of the file and uncomment
// // the "TAGGED VALUES" part and have op.rs continue to work. If we knew we would only have
// // the untagged usize representation we could simplify op.rs to remove the untag() and
// // into() calls and get rid of this code.

// // "extension trait" to get .untag() as a method of usize
// pub trait Untag { fn untag<X: ValueType>(self) -> X; }
// impl Untag for usize {
//     fn untag<X: ValueType>(self) -> X {
//         let Ok(v) = X::from_value(self);
//         v
//     }
// }

// pub trait ValueType: Sized {
//     fn to_value(self) -> Value;
//     fn from_value(value: Value) -> Result<Self, Infallible>;
// }

// impl ValueType for usize {
//     fn to_value(self) -> usize { self }
//     fn from_value(value: usize) -> Result<usize, Infallible> { Ok(value) }
// }

// // ---------- END GARBAGE CODE ----------


// // ===============================================================================
// // ================================ TAGGED VALUES ================================
// // ===============================================================================

// use std::rc::Rc;

// #[derive(Clone, Hash, PartialOrd, Ord, PartialEq, Eq, Debug)]
// pub enum Value { Int(usize), Str(Rc<String>) }

// impl From<usize> for Value { fn from(x: usize) -> Value { Value::Int(x) } }
// impl From<&Rc<String>> for Value { fn from(x: &Rc<String>) -> Value { Value::Str(x.clone()) } }
// impl From<Rc<String>> for Value { fn from(x: Rc<String>) -> Value { Value::Str(x) } }

// #[derive(PartialEq,Eq,PartialOrd,Ord,Clone,Copy,Debug)]
// pub enum Tag { Int, Str }
// #[derive(PartialEq,Eq,PartialOrd,Ord,Clone,Copy,Debug)]
// pub struct TagError { expected: Tag, actual: Tag, }

// impl Value {
//     pub fn tag(&self) -> Tag {
//         match self {
//             Value::Int(_) => Tag::Int,
//             Value::Str(_) => Tag::Str,
//         }
//     }

//     pub fn untag<X: ValueType>(&self) -> X {
//         match X::try_from(self) {
//             Ok(v) => v,
//             Err(e) => panic!("tag error: {e:?}"),
//         }
//     }
// }

// pub trait ValueType: Sized + for<'a> TryFrom<&'a Value, Error = TagError>
// {
//     fn tag() -> Tag;
//     fn to_value(self) -> Value;
//     fn from_value(value: Value) -> Result<Self, TagError> {
//         TryFrom::try_from(&value)
//     }
// }

// // TODO: macro-generate this & other cases.
// impl TryFrom<&Value> for usize {
//     type Error = TagError;
//     fn try_from(value: &Value) -> Result<usize, TagError> {
//         match value {
//             Value::Int(n) => Ok(*n),
//             _ => Err(TagError { expected: Tag::Int, actual: value.tag() }),
//         }
//     }
// }

// impl ValueType for usize {
//     fn tag() -> Tag { Tag::Int }
//     fn to_value(self) -> Value { Value::Int(self) }
//     // fn from_value(value: Value) -> Result<usize, TagError> {
//     //     match value {
//     //         Value::Int(n) => Ok(n),
//     //         _ => Err(TagError { expected: Tag::Int, actual: value.tag() })
//     //     }
//     // }
// }
