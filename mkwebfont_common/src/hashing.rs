use std::hash::{BuildHasher, Hash, Hasher};
use wyrand::WyHash;

// we don't need a secret, and generating a secret involves primality checks. oww.
// thus, new_with_default_secret

#[derive(Copy, Clone, Debug, Default)]
pub struct WyHashBuilder;
impl BuildHasher for WyHashBuilder {
    type Hasher = WyHash;
    fn build_hasher(&self) -> Self::Hasher {
        WyHash::new_with_default_secret(0xfc1abcacd1fc58fe)
    }
}

pub fn wyhash(seed: u64, data: &(impl Hash + ?Sized)) -> u64 {
    let mut wyh = WyHash::new_with_default_secret(seed);
    data.hash(&mut wyh);
    wyh.finish()
}

pub use wyrand::WyRand;

///////////////////////////////////////////////////////////////////////////////
// Code from <https://github.com/kolloch/nix-base32>

// omitted: E O U T
const BASE32_CHARS: &[u8] = b"0123456789abcdfghijklmnpqrsvwxyz";

/// Converts the given byte slice to a nix-compatible base32 encoded String.
pub fn to_nix_base32(bytes: &[u8]) -> String {
    let len = (bytes.len() * 8 - 1) / 5 + 1;

    (0..len)
        .rev()
        .map(|n| {
            let b: usize = (n as usize) * 5;
            let i: usize = b / 8;
            let j: usize = b % 8;
            // bits from the lower byte
            let v1 = bytes[i].checked_shr(j as u32).unwrap_or(0);
            // bits from the upper byte
            let v2 = if i >= bytes.len() - 1 {
                0
            } else {
                bytes[i + 1].checked_shl(8 - j as u32).unwrap_or(0)
            };
            let v: usize = (v1 | v2) as usize;
            char::from(BASE32_CHARS[v % BASE32_CHARS.len()])
        })
        .collect()
}

// end code from nix-base32
///////////////////////////////////////////////////////////////////////////////

pub fn hash_full(data: &[u8]) -> String {
    let blake3_hash = blake3::hash(data);
    let hash_str = to_nix_base32(&*blake3_hash.as_bytes());
    hash_str.to_string()
}

pub fn hash_fragment(data: &[u8]) -> String {
    let blake3_hash = blake3::hash(data);
    let hash_str = to_nix_base32(&*blake3_hash.as_bytes());
    let hash_str = &hash_str[1..21];
    hash_str.to_string()
}
