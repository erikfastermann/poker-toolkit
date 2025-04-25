use std::fmt;

use rand::{distributions::{Distribution, Standard}, Rng};

use crate::result::Result;

#[repr(i8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Rank {
    Two = 0,
    Three = 1,
    Four = 2,
    Five = 3,
    Six = 4,
    Seven = 5,
    Eight = 6,
    Nine = 7,
    Ten = 8,
    Jack = 9,
    Queen = 10,
    King = 11,
    Ace = 12,
}

use Rank::*;

impl Distribution<Rank> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Rank {
        let n = rng.gen_range(0..i8::try_from(Rank::COUNT).unwrap());
        Rank::try_from(n).unwrap()
    }
}

impl fmt::Display for Rank {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rank = match *self {
            Two => "2",
            Three => "3",
            Four => "4",
            Five => "5",
            Six => "6",
            Seven => "7",
            Eight => "8",
            Nine => "9",
            Ten => "T",
            Jack => "J",
            Queen => "Q",
            King => "K",
            Ace => "A",
        };
        write!(f, "{}", rank)
    }
}

impl TryFrom<i8> for Rank {
    type Error = ();

    fn try_from(v: i8) -> std::result::Result<Self, Self::Error> {
        match v {
            x if x == Two as i8 => Ok(Two),
            x if x == Three as i8 => Ok(Three),
            x if x == Four as i8 => Ok(Four),
            x if x == Five as i8 => Ok(Five),
            x if x == Six as i8 => Ok(Six),
            x if x == Seven as i8 => Ok(Seven),
            x if x == Eight as i8 => Ok(Eight),
            x if x == Nine as i8 => Ok(Nine),
            x if x == Ten as i8 => Ok(Ten),
            x if x == Jack as i8 => Ok(Jack),
            x if x == Queen as i8 => Ok(Queen),
            x if x == King as i8 => Ok(King),
            x if x == Ace as i8 => Ok(Ace),
            _ => Err(()),
        }
    }
}

impl Rank {
    pub const COUNT: usize = 13;

    pub const RANKS: [Rank; Rank::COUNT] = [
        Two,
        Three,
        Four,
        Five,
        Six,
        Seven,
        Eight,
        Nine,
        Ten,
        Jack,
        Queen,
        King,
        Ace,
    ];

    pub fn to_i8(self) -> i8 {
        self as i8
    }

    pub fn to_u8(self) -> u8 {
        self as u8
    }

    pub fn to_i16(self) -> i16 {
        self.to_i8().into()
    }

    pub fn to_u16(self) -> u16 {
        self.to_u8().into()
    }

    pub fn to_u32(self) -> u32 {
        self.to_u8().into()
    }

    pub fn to_usize(self) -> usize {
        self.to_u8().into()
    }

    pub fn from_ascii(char: u8) -> Result<Self> {
        let rank = match char {
            b'2' => Two,
            b'3' => Three,
            b'4' => Four,
            b'5' => Five,
            b'6' => Six,
            b'7' => Seven,
            b'8' => Eight,
            b'9' => Nine,
            b'T' => Ten,
            b'J' => Jack,
            b'Q' => Queen,
            b'K' => King,
            b'A' => Ace,
            _ => return Err(format!("invalid rank char '{}'", char::from(char)).into()),
        };
        Ok(rank)
    }

    pub fn range(from: Rank, to: Rank) -> impl Iterator<Item = Rank> {
        (from.to_i8()..=to.to_i8()).map(|item| item.try_into().unwrap())
    }

    pub fn predecessor(self) -> Option<Rank> {
        match self {
            Two => None,
            Three => Some(Two),
            Four => Some(Three),
            Five => Some(Four),
            Six => Some(Five),
            Seven => Some(Six),
            Eight => Some(Seven),
            Nine => Some(Eight),
            Ten => Some(Nine),
            Jack => Some(Ten),
            Queen => Some(Jack),
            King => Some(Queen),
            Ace => Some(King),
        }
    }

    pub fn successor(self) -> Option<Rank> {
        match self {
            Two => Some(Three),
            Three => Some(Four),
            Four => Some(Five),
            Five => Some(Six),
            Six => Some(Seven),
            Seven => Some(Eight),
            Eight => Some(Nine),
            Nine => Some(Ten),
            Ten => Some(Jack),
            Jack => Some(Queen),
            Queen => Some(King),
            King => Some(Ace),
            Ace => None,
        }
    }
}
