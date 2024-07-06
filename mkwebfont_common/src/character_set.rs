use crate::hashing::WyHashSet;
use bincode::{Decode, Encode};
use std::fmt::{Debug, Formatter};

#[derive(Clone, Eq, PartialEq, Default)]
pub struct CharacterSet(WyHashSet<u32>);
impl CharacterSet {
    pub fn new() -> Self {
        Self::default()
    }
}
impl Debug for CharacterSet {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "[set of {} characters]", self.0.len())
    }
}

#[derive(Clone, Debug, Encode, Decode)]
pub struct CompressedCharacterSet(Vec<u32>);
