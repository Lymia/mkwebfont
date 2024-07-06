use crate::hashing::{WyHashBuilder, WyHashSet};
use bincode::{Decode, Encode};
use std::{
    collections::hash_set::{IntoIter, Iter},
    fmt::{Debug, Formatter},
    ops::{BitAnd, BitOr, BitXor, Sub},
};

#[derive(Clone, Eq, PartialEq)]
pub struct CharacterSet(WyHashSet<u32>);
impl CharacterSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, character: u32) -> bool {
        self.0.insert(character)
    }

    pub fn remove(&mut self, character: u32) -> bool {
        self.0.remove(&character)
    }

    pub fn contains(&self, character: u32) -> bool {
        self.0.contains(&character)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns an iterator.
    pub fn iter(&self) -> CharacterSetIter {
        CharacterSetIter(self.0.iter())
    }

    /// Converts this into an iterator.
    pub fn into_iter(self) -> CharacterSetIntoIter {
        CharacterSetIntoIter(self.0.into_iter())
    }

    /// Decompresses the character set.
    pub fn decompress(data: &CompressedCharacterSet) -> Self {
        let iter = data.0.iter().cloned().take(1).chain(
            data.0
                .iter()
                .skip(1)
                .zip(data.0.iter())
                .map(|(cur, prev)| *cur - *prev),
        );
        CharacterSet(iter.collect())
    }

    /// Compresses a character set.
    pub fn compressed(&self) -> CompressedCharacterSet {
        let mut sorted: Vec<_> = self.iter().collect();
        sorted.sort();

        if sorted.len() >= 2 {
            for i in (1..sorted.len()).rev() {
                sorted[i] -= sorted[i - 1];
            }
        }

        CompressedCharacterSet(sorted)
    }
}
impl Default for CharacterSet {
    fn default() -> Self {
        CharacterSet(WyHashSet::with_capacity_and_hasher(128, WyHashBuilder::default()))
    }
}
impl Debug for CharacterSet {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "[set of {} characters]", self.0.len())
    }
}
impl IntoIterator for CharacterSet {
    type Item = u32;
    type IntoIter = CharacterSetIntoIter;
    fn into_iter(self) -> Self::IntoIter {
        self.into_iter()
    }
}
impl<'a> IntoIterator for &'a CharacterSet {
    type Item = u32;
    type IntoIter = CharacterSetIter<'a>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl Extend<u32> for CharacterSet {
    fn extend<T: IntoIterator<Item = u32>>(&mut self, iter: T) {
        self.0.extend(iter)
    }
}

impl<'a> BitAnd<&'a CharacterSet> for CharacterSet {
    type Output = CharacterSet;
    fn bitand(mut self, rhs: &'a CharacterSet) -> Self::Output {
        self.0.retain(|x| rhs.contains(*x));
        self
    }
}
impl<'a> BitOr<&'a CharacterSet> for CharacterSet {
    type Output = CharacterSet;
    fn bitor(mut self, rhs: &'a CharacterSet) -> Self::Output {
        for char in rhs {
            self.insert(char);
        }
        self
    }
}
impl<'a> BitXor<&'a CharacterSet> for CharacterSet {
    type Output = CharacterSet;
    fn bitxor(mut self, rhs: &'a CharacterSet) -> Self::Output {
        for char in rhs {
            if !self.contains(char) {
                self.insert(char);
            } else {
                self.remove(char);
            }
        }
        self
    }
}
impl<'a> Sub<&'a CharacterSet> for CharacterSet {
    type Output = CharacterSet;
    fn sub(mut self, rhs: &'a CharacterSet) -> Self::Output {
        for char in rhs {
            self.remove(char);
        }
        self
    }
}

macro_rules! bitops {
    ($trait_name:ident, $trait_func:ident) => {
        impl $trait_name for CharacterSet {
            type Output = CharacterSet;
            fn $trait_func(self, rhs: CharacterSet) -> Self::Output {
                $trait_name::$trait_func(self, &rhs)
            }
        }
        impl<'a> $trait_name<CharacterSet> for &'a CharacterSet {
            type Output = CharacterSet;
            fn $trait_func(self, rhs: CharacterSet) -> Self::Output {
                $trait_name::$trait_func(self.clone(), &rhs)
            }
        }
        impl<'a, 'b> $trait_name<&'a CharacterSet> for &'b CharacterSet {
            type Output = CharacterSet;
            fn $trait_func(self, rhs: &'a CharacterSet) -> Self::Output {
                $trait_name::$trait_func(self.clone(), rhs)
            }
        }
    };
}
bitops!(BitAnd, bitand);
bitops!(BitOr, bitor);
bitops!(BitXor, bitxor);
bitops!(Sub, sub);

/// The iterator for [`CharacterSet`]
pub struct CharacterSetIter<'a>(Iter<'a, u32>);
impl<'a> Iterator for CharacterSetIter<'a> {
    type Item = u32;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|x| *x)
    }
}

/// The owned iterator for [`CharacterSet`]
pub struct CharacterSetIntoIter(IntoIter<u32>);
impl Iterator for CharacterSetIntoIter {
    type Item = u32;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

/// A compressed character set.
///
/// This does not take less memory, but compresses better, as it is delta encoded and not in a
/// random order as a [`HashSet`] would be.
#[derive(Clone, Debug, Encode, Decode)]
pub struct CompressedCharacterSet(Vec<u32>);
