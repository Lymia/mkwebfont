use std::fmt::{Debug, Formatter};
use bincode::{Decode, Encode};
use crate::hashing::WyHashSet;

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