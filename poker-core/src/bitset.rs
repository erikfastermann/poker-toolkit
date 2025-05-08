use std::ops::BitAnd;

#[derive(Debug, Clone, Copy, PartialEq, Eq)] // TODO: Custom Debug
pub struct Bitset<const SIZE: usize>([u8; SIZE]);

impl<const SIZE: usize> Bitset<SIZE> {
    pub const EMPTY: Self = Self([0; SIZE]);

    pub fn ones(n: usize) -> Self {
        let mut s = Self::EMPTY;
        for i in 0..n {
            s.set(i);
        }
        s
    }

    pub fn set(&mut self, index: usize) {
        self.0[index / 8] |= 1 << (index % 8);
    }

    pub fn with(mut self, index: usize) -> Self {
        self.set(index);
        self
    }

    pub fn remove(&mut self, index: usize) {
        self.0[index / 8] &= !(1 << (index % 8));
    }

    pub fn has(&self, index: usize) -> bool {
        let i = index / 8;
        if i >= self.0.len() {
            false
        } else {
            (self.0[i] & (1 << (index % 8))) != 0
        }
    }

    pub fn iter(&self, max_exclusive: usize) -> impl Iterator<Item = usize> + '_ {
        (0..max_exclusive)
            .into_iter()
            .filter(|index| self.has(*index))
    }

    pub fn count(&self) -> u32 {
        self.0.iter().map(|n| n.count_ones()).sum::<u32>()
    }
}

impl<const SIZE: usize> BitAnd for Bitset<SIZE> {
    type Output = Self;

    fn bitand(mut self, rhs: Self) -> Self::Output {
        for (a, b) in self.0.iter_mut().zip(rhs.0) {
            *a &= b;
        }
        self
    }
}
