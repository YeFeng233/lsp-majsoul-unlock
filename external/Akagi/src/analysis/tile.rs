//! Tile representation for the analysis engine.
//!
//! Uses the same 0..34 indexing as `riichienv-core::types`:
//!   0..=8   = 1m..9m
//!   9..=17  = 1p..9p
//!   18..=26 = 1s..9s
//!   27..=33 = E,S,W,N,P,F,C  (1z..7z)

use riichienv_core::parser;
use serde::{Deserialize, Serialize};

pub const TILE_COUNT: usize = 34;
pub const SUIT_M: u8 = 0;
pub const SUIT_P: u8 = 9;
pub const SUIT_S: u8 = 18;
pub const HONOR_BASE: u8 = 27;
pub const HONOR_E: u8 = 27;
pub const HONOR_S: u8 = 28;
pub const HONOR_W: u8 = 29;
pub const HONOR_N: u8 = 30;
pub const HONOR_P: u8 = 31;
pub const HONOR_F: u8 = 32;
pub const HONOR_C: u8 = 33;

/// Tile in 34-space.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct Tile34(pub u8);

impl Tile34 {
    pub fn new(idx: u8) -> Option<Self> {
        if (idx as usize) < TILE_COUNT {
            Some(Tile34(idx))
        } else {
            None
        }
    }

    pub fn idx(self) -> u8 {
        self.0
    }

    pub fn is_man(self) -> bool {
        self.0 < SUIT_P
    }
    pub fn is_pin(self) -> bool {
        self.0 >= SUIT_P && self.0 < SUIT_S
    }
    pub fn is_sou(self) -> bool {
        self.0 >= SUIT_S && self.0 < HONOR_BASE
    }
    pub fn is_honor(self) -> bool {
        self.0 >= HONOR_BASE
    }
    pub fn is_suit(self) -> bool {
        !self.is_honor()
    }
    pub fn is_wind(self) -> bool {
        (HONOR_E..=HONOR_N).contains(&self.0)
    }
    pub fn is_dragon(self) -> bool {
        (HONOR_P..=HONOR_C).contains(&self.0)
    }

    /// 1..=9 for suits, 1..=7 for honors.
    pub fn number(self) -> u8 {
        if self.is_honor() {
            self.0 - HONOR_BASE + 1
        } else {
            self.0 % 9 + 1
        }
    }

    /// Terminal = 1 or 9 in suits.
    pub fn is_terminal(self) -> bool {
        self.is_suit() && (self.number() == 1 || self.number() == 9)
    }

    /// Yaochuhai = terminal or honor.
    pub fn is_yaochuhai(self) -> bool {
        self.is_terminal() || self.is_honor()
    }

    /// Suit base index (0 / 9 / 18) or 27 for honors.
    pub fn suit_base(self) -> u8 {
        if self.is_man() {
            SUIT_M
        } else if self.is_pin() {
            SUIT_P
        } else if self.is_sou() {
            SUIT_S
        } else {
            HONOR_BASE
        }
    }

    /// Next tile within the same group, used for dora indicator → dora rule.
    /// Wraps within the suit (9m → 1m), within winds (N → E), and within dragons (C → P).
    pub fn dora_next(self) -> Tile34 {
        let n = self.number();
        if self.is_suit() {
            let base = self.suit_base();
            Tile34(base + (n % 9))
        } else if self.is_wind() {
            // E S W N → S W N E
            Tile34(HONOR_E + ((self.0 - HONOR_E + 1) % 4))
        } else {
            // P F C → F C P
            Tile34(HONOR_P + ((self.0 - HONOR_P + 1) % 3))
        }
    }

    /// Parse a mjai-style tile string (`"1m"`, `"5mr"`, `"E"`, `"P"`, `"3z"`).
    /// Red-five flag is dropped — Tile34 is a 34-space index only.
    pub fn from_mjai(s: &str) -> Option<Tile34> {
        let trimmed = if let Some(stripped) = s.strip_suffix('r') {
            stripped
        } else {
            s
        };
        let tid = parser::mjai_to_tid(trimmed).or_else(|| parser::mjai_to_tid(s))?;
        let idx = tid_to_tile34(tid);
        Tile34::new(idx)
    }

    /// Render in mjai short form (no red flag — analysis is colourblind here).
    pub fn to_mjai(self) -> &'static str {
        TILE_NAMES[self.0 as usize]
    }
}

const TILE_NAMES: [&str; TILE_COUNT] = [
    "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p", "5p", "6p", "7p",
    "8p", "9p", "1s", "2s", "3s", "4s", "5s", "6s", "7s", "8s", "9s", "E", "S", "W", "N", "P", "F",
    "C",
];

/// Convert a 0..136 tile id (riichienv form) to 0..34.
/// Red-five sentinels (16, 52, 88) collapse to the corresponding 5.
pub fn tid_to_tile34(tid: u8) -> u8 {
    match tid {
        16 => 4,  // 5mr → 5m
        52 => 13, // 5pr → 5p
        88 => 22, // 5sr → 5s
        _ => tid / 4,
    }
}

/// Whether a 136-tile id is a red five.
pub fn is_aka(tid: u8) -> bool {
    tid == 16 || tid == 52 || tid == 88
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mjai_round_trip() {
        for (idx, name) in TILE_NAMES.iter().enumerate() {
            let t = Tile34::from_mjai(name).unwrap();
            assert_eq!(t.idx() as usize, idx, "{name}");
            assert_eq!(t.to_mjai(), *name);
        }
    }

    #[test]
    fn red_five_collapses() {
        assert_eq!(Tile34::from_mjai("5mr").unwrap().idx(), 4);
        assert_eq!(Tile34::from_mjai("5pr").unwrap().idx(), 13);
        assert_eq!(Tile34::from_mjai("5sr").unwrap().idx(), 22);
    }

    #[test]
    fn classification() {
        assert!(Tile34::from_mjai("1m").unwrap().is_terminal());
        assert!(!Tile34::from_mjai("5m").unwrap().is_terminal());
        assert!(Tile34::from_mjai("E").unwrap().is_wind());
        assert!(Tile34::from_mjai("P").unwrap().is_dragon());
        assert!(Tile34::from_mjai("9s").unwrap().is_yaochuhai());
        assert!(!Tile34::from_mjai("4p").unwrap().is_yaochuhai());
    }

    #[test]
    fn dora_next_wraps() {
        assert_eq!(Tile34::from_mjai("1m").unwrap().dora_next().to_mjai(), "2m");
        assert_eq!(Tile34::from_mjai("9m").unwrap().dora_next().to_mjai(), "1m");
        assert_eq!(Tile34::from_mjai("9p").unwrap().dora_next().to_mjai(), "1p");
        assert_eq!(Tile34::from_mjai("9s").unwrap().dora_next().to_mjai(), "1s");
        assert_eq!(Tile34::from_mjai("E").unwrap().dora_next().to_mjai(), "S");
        assert_eq!(Tile34::from_mjai("N").unwrap().dora_next().to_mjai(), "E");
        assert_eq!(Tile34::from_mjai("P").unwrap().dora_next().to_mjai(), "F");
        assert_eq!(Tile34::from_mjai("C").unwrap().dora_next().to_mjai(), "P");
    }

    #[test]
    fn tid_to_tile34_handles_aka() {
        assert_eq!(tid_to_tile34(0), 0); // 1m
        assert_eq!(tid_to_tile34(16), 4); // 5mr → 5m
        assert_eq!(tid_to_tile34(20), 5); // 6m
        assert_eq!(tid_to_tile34(52), 13);
        assert_eq!(tid_to_tile34(88), 22);
        assert_eq!(tid_to_tile34(135), 33); // C
    }
}
