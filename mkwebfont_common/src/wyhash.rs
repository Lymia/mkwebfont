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

pub fn wyhash(seed: u64, data: &[u8]) -> u64 {
    let mut wyh = WyHash::new_with_default_secret(seed);
    data.hash(&mut wyh);
    wyh.finish()
}
