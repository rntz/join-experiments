// ========== REPRESENTING VALUES ==========
//
// There are at least three reasonable representation choices:
//
// 1. TAG EVERYTHING -- Tag every value; pay tagging overhead in space and time.
//
// 2. INTERNING -- Everything is usize. Intern anything that can't fit in usize.
//
// 3. SMART TAGGING -- Tag intelligently, e.g. per-column. Exploit this by processing
// whole batches of data per tag check instead of doing one check per value touched.
//
// (1) is simplest but probably slowest.
//
// (2) requires carefully managing interning tables.
//
// (3) requires a careful, thoughtful redesign of the algorithmic core (the Trie index
// data structure & join implementation).
//
// I've implemented both (1) and (2) with the same interface. However, I haven't yet
// implemented interning/deinterning. You can choose which to use by switching which line
// is uncommented:

pub use tagged::*;         // tagged representation (strategy 1)
// pub use usize::*;          // usize representation (strategy 2)

// Once you have reasonable real-world benchmarks, I suggest comparing the performance of
// (1) and (2). If the performance of (1) is acceptable, use it, because it keeps the code
// simple and the data self-describing rather than requiring interning tables everywhere.
//
// Because every operator call has to do tag-checking, I expect (1) to perform badly for
// operator-heavy queries. I have some evidence for this: the "operator-filtered triangle"
// benchmark in triangles.rs, which calculates {E(x,y), E(y,z), E(x,z), x <= y, y <= z},
// is 1.18x slower with tagging. But an 18% slowdown ain't that bad.
//
// On the other hand, under (2), operators over interned data (eg strings) need to pay the
// cost of doing intern table lookups. Haven't implemented this, so I don't know how bad
// it is.
//
// (3) might give best performance overall, but is hardest to implement.

// ========== TO PARAMETERIZE or NOT TO PARAMETERIZE ==========
//
// Instead of switching Value at the module level, we could parameterize all relevant code
// (Database, Query, Operator, etc) over a Value type. Query is already parameterized over
// variable id, relation id, and operator representations. On the one hand, one more
// parameter doesn't seem that bad; on the other, three parameters is already unwieldy.

// ========== INTERNING IS HARD BECAUSE OF GARBAGE COLLECTION ==========
//
// First note: we should only intern things that need a pointer or don't fit in a usize.
// On Wasm, usize = 32 bits. Strings: intern, they're pointers ; doubles -> intern,
// they're 64 bits; 32-bit integers, unsigned integers, floats: don't intern, just convert
// to/from usize.
//
// Second note: operators over interned types will need to de-intern their arguments and
// intern their result.
//
// The main problem with interning is freeing things (removing them from the intern
// table). To do this you need to know when they're not used any more. For a static
// database this is relatively easy: the database gets its own intern table.
//
// The only wrinkle is that when running a query, operators that produce an interned type
// need somewhere to intern it. The result won't outlive the query, so: a query gets a
// temporary intern table, separate from the database's intern table. Deallocate at query
// end after un-interning the materialized results.
//
// If the database changes over time, however, we must track which interned values are
// still referred to by some row in the DB. One strategy: from the database schema, we can
// derive a query for each intern type, then maintain this query incrementally over
// updates; the diffs in the result of the query tell us what to delete from the intern
// table.


// ==================== TAGGED ENUM (strategy 1) ====================
pub mod tagged {
    use std::rc::Rc;

    #[derive(Clone, Hash, PartialOrd, Ord, PartialEq, Eq, Debug)]
    pub enum Value { Int(usize), Str(Rc<String>) }

    impl From<usize> for Value { fn from(x: usize) -> Value { Value::Int(x) } }
    impl From<&Rc<String>> for Value { fn from(x: &Rc<String>) -> Value { Value::Str(x.clone()) } }
    impl From<Rc<String>> for Value { fn from(x: Rc<String>) -> Value { Value::Str(x) } }

    #[derive(PartialEq,Eq,PartialOrd,Ord,Clone,Copy,Debug)]
    pub enum Tag { Int, Str }
    #[derive(PartialEq,Eq,PartialOrd,Ord,Clone,Copy,Debug)]
    pub struct TagError { expected: Tag, actual: Tag, }

    impl Value {
        pub fn tag(&self) -> Tag {
            match self {
                Value::Int(_) => Tag::Int,
                Value::Str(_) => Tag::Str,
            }
        }

        pub fn untag<X: ValueType>(&self) -> X {
            match X::from_value(self) {
                Ok(v) => v,
                Err(e) => panic!("tag error: {e:?}"),
            }
        }
    }

    pub trait ValueType: Sized {
        fn tag() -> Tag;
        fn to_value(self) -> Value;
        fn from_value(value: &Value) -> Result<Self, TagError>;
    }

    impl ValueType for usize {
        fn tag() -> Tag { Tag::Int }
        fn to_value(self) -> Value { Value::Int(self) }
        fn from_value(value: &Value) -> Result<usize, TagError> {
            match value {
                Value::Int(n) => Ok(*n),
                _ => Err(TagError { expected: Tag::Int, actual: value.tag() })
            }
        }
    }
}


// ==================== STRUCT of USIZE (strategy 2) ====================
pub mod usize {
    #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct Value(usize);

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
        fn from_value(value: &Value) -> Result<Self, TagError>;
    }
    pub type TagError = Infallible; // no tag errors exist in this representation.

    impl ValueType for usize {
        fn to_value(self) -> Value { Value(self) }
        fn from_value(value: &Value) -> Result<usize, TagError> { Ok(value.0) }
    }

    // Convenience method for tag-checking & unwrapping.
    impl Value {
        pub fn untag<X: ValueType>(&self) -> X { let Ok(v) = X::from_value(self); v }
    }
}
