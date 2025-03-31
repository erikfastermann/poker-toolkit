use std::fmt;

use rand::{
    distributions::{Distribution, Standard},
    Rng,
};

use crate::result::Result;

#[repr(i8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Suite {
    Diamonds = 0,
    Spades = 1,
    Hearts = 2,
    Clubs = 3,
}

use Suite::*;

impl Distribution<Suite> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Suite {
        let n = rng.gen_range(0..i8::try_from(Suite::COUNT).unwrap());
        Suite::try_from(n).unwrap()
    }
}

impl fmt::Display for Suite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let suite = match *self {
            Diamonds => "d",
            Spades => "s",
            Hearts => "h",
            Clubs => "c",
        };
        write!(f, "{}", suite)
    }
}

impl TryFrom<i8> for Suite {
    type Error = ();

    fn try_from(v: i8) -> std::result::Result<Self, Self::Error> {
        match v {
            x if x == Diamonds as i8 => Ok(Diamonds),
            x if x == Spades as i8 => Ok(Spades),
            x if x == Hearts as i8 => Ok(Hearts),
            x if x == Clubs as i8 => Ok(Clubs),
            _ => Err(()),
        }
    }
}

impl Suite {
    pub const COUNT: usize = 4;

    pub const SUITES: [Suite; Self::COUNT] = [Diamonds, Spades, Hearts, Clubs];

    pub fn from_ascii(ch: u8) -> Result<Self> {
        let suite = match ch {
            b'd' => Diamonds,
            b's' => Spades,
            b'h' => Hearts,
            b'c' => Clubs,
            _ => return Err(format!("invalid suite char '{}'", char::from(ch)).into()),
        };
        Ok(suite)
    }

    fn to_i8(self) -> i8 {
        self as i8
    }

    fn to_u8(self) -> u8 {
        self as u8
    }

    pub fn to_usize(self) -> usize {
        self.to_u8().into()
    }

    pub fn to_index(self) -> i8 {
        let index = self.to_i8() * 16;
        debug_assert!(index >= 0 && index < 64);
        index
    }

    pub fn to_index_u64(self) -> u64 {
        self.to_index() as u64
    }

    pub fn to_symbol_str(self) -> &'static str {
        match self {
            Diamonds => "♦",
            Spades => "♠",
            Hearts => "♥",
            Clubs => "♣",
        }
    }
}
