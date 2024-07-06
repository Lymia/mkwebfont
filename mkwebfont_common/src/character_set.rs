use bincode::{Decode, Encode};
use std::{
    collections::{
        btree_set::{IntoIter, Iter},
        BTreeSet,
    },
    fmt::{Debug, Formatter},
    ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, BitXorAssign, Sub, SubAssign},
};

#[derive(Clone, Eq, PartialEq, Default)]
pub struct CharacterSet(BTreeSet<u32>);
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

    pub fn min(&self) -> Option<u32> {
        self.0.iter().min().cloned()
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
        let mut data: Vec<_> = self.iter().collect();
        if data.len() >= 2 {
            for i in (1..data.len()).rev() {
                data[i] -= data[i - 1];
            }
        }

        CompressedCharacterSet(data)
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

impl<'a> BitAndAssign<&'a CharacterSet> for CharacterSet {
    fn bitand_assign(&mut self, rhs: &'a CharacterSet) {
        self.0.retain(|x| rhs.contains(*x));
    }
}
impl<'a> BitOrAssign<&'a CharacterSet> for CharacterSet {
    fn bitor_assign(&mut self, rhs: &'a CharacterSet) {
        for char in rhs {
            self.insert(char);
        }
    }
}
impl<'a> BitXorAssign<&'a CharacterSet> for CharacterSet {
    fn bitxor_assign(&mut self, rhs: &'a CharacterSet) {
        for char in rhs {
            if !self.contains(char) {
                self.insert(char);
            } else {
                self.remove(char);
            }
        }
    }
}
impl<'a> SubAssign<&'a CharacterSet> for CharacterSet {
    fn sub_assign(&mut self, rhs: &'a CharacterSet) {
        for char in rhs {
            self.remove(char);
        }
    }
}

macro_rules! bitops {
    ($trait_name:ident, $trait_func:ident, $assign_trait:ident, $assign_func:ident) => {
        impl $assign_trait for CharacterSet {
            fn $assign_func(&mut self, rhs: CharacterSet) {
                $assign_trait::$assign_func(self, &rhs);
            }
        }
        impl<'a> $assign_trait<CharacterSet> for &'a mut CharacterSet {
            fn $assign_func(&mut self, rhs: CharacterSet) {
                $assign_trait::$assign_func(*self, &rhs);
            }
        }
        impl<'a, 'b> $assign_trait<&'a CharacterSet> for &'b mut CharacterSet {
            fn $assign_func(&mut self, rhs: &'a CharacterSet) {
                $assign_trait::$assign_func(*self, rhs);
            }
        }
        impl $trait_name for CharacterSet {
            type Output = CharacterSet;
            fn $trait_func(self, rhs: CharacterSet) -> Self::Output {
                $trait_name::$trait_func(self, &rhs)
            }
        }
        impl<'a> $trait_name<&'a CharacterSet> for CharacterSet {
            type Output = CharacterSet;
            fn $trait_func(mut self, rhs: &'a CharacterSet) -> Self::Output {
                $assign_trait::$assign_func(&mut self, rhs);
                self
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
bitops!(BitAnd, bitand, BitAndAssign, bitand_assign);
bitops!(BitOr, bitor, BitOrAssign, bitor_assign);
bitops!(BitXor, bitxor, BitXorAssign, bitxor_assign);
bitops!(Sub, sub, SubAssign, sub_assign);

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
#[derive(Clone, Encode, Decode)]
pub struct CompressedCharacterSet(Vec<u32>);
impl Debug for CompressedCharacterSet {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "[compressed set of {} characters]", self.0.len())
    }
}
