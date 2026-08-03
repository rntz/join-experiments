// (Mostly Claude-generated with minimal review.)
use std::collections::{HashMap, HashSet};

// ---------- HASHER SELECTION ----------
//
// Hash algorithm impacts join performance heavily (factor of ~3x). Rust's default is slow
// in exchange for extra security against adversarial attacks. We probably don't need
// this? So we use a much simpler, faster hash, FxHash. Would be fine to replace this with
// a library, presumably there's some crate in the Rust ecosystem for this.
//
// Pick your hash algorithm by changing "type HashBuilder":

pub type HashBuilder = FxBuildHasher; // fast, non-cryptographic hash
// pub type HashBuilder = std::collections::hash_map::RandomState; // stdlib SipHash

// Aliases used by rest of crate.
pub type Map<K, V> = HashMap<K, V, HashBuilder>;
pub type Set<K> = HashSet<K, HashBuilder>;

// An implementation of FxHash (Claude-generated).
const FX_SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;
const FX_ROTATE: u32 = 5;

// One issue with FxHash is that zeros always hash to zero; eg all Vecs containing only
// zeros will hash-collide. So hashing variable-length structures is a bit dangerous.
// Fortunately we don't do that. (Potential fix: make the initial hash 1.)
#[derive(Default)]
pub struct FxHasher {
    hash: u64,
}

impl FxHasher {
    #[inline]
    fn add(&mut self, i: u64) {
        self.hash = (self.hash.rotate_left(FX_ROTATE) ^ i).wrapping_mul(FX_SEED);
    }
}

impl std::hash::Hasher for FxHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(i as u64);
    }
    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(i as u64);
    }
    #[inline]
    fn write(&mut self, mut bytes: &[u8]) {
        while bytes.len() >= 8 {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(&bytes[..8]);
            self.add(u64::from_le_bytes(buf));
            bytes = &bytes[8..];
        }
        if !bytes.is_empty() {
            let mut buf = [0u8; 8];
            buf[..bytes.len()].copy_from_slice(bytes);
            self.add(u64::from_le_bytes(buf));
        }
    }
}

#[derive(Default, Clone)]
pub struct FxBuildHasher;
impl std::hash::BuildHasher for FxBuildHasher {
    type Hasher = FxHasher;
    #[inline]
    fn build_hasher(&self) -> FxHasher {
        FxHasher::default()
    }
}
