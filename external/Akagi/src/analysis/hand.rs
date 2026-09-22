//! Input data for the analysis engine.
//!
//! `PlayerInfo34` is the closed-form input: own hand as 34-counts,
//! own melds, opponent observations, dora, and round/seat winds.
//! It is the analogue of mahjong-helper's `model.PlayerInfo`.
//!
//! All tile indices use the 0..34 layout from [`super::tile`].

use serde::{Deserialize, Serialize};

use super::tile::{Tile34, TILE_COUNT};

/// 34-space tile count vector.
pub type Counts34 = [u8; TILE_COUNT];

/// Concealed (`Ankan`) vs open (`Chi`/`Pon`/`Daiminkan`/`Kakan`) call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Meld34Kind {
    Chi,
    Pon,
    Daiminkan,
    Ankan,
    Kakan,
}

impl Meld34Kind {
    pub fn is_kan(self) -> bool {
        matches!(self, Self::Daiminkan | Self::Ankan | Self::Kakan)
    }
    pub fn is_concealed(self) -> bool {
        matches!(self, Self::Ankan)
    }
}

/// One called/declared meld.
///
/// `tiles` lists the 34-space indices that make up the meld:
///   - Chi: 3 distinct tiles
///   - Pon: 1 tile (×3 implied)
///   - Kan: 1 tile (×4 implied)
///
/// `called_tile` is the tile taken from another player (for open melds).
/// `aka_count` is the number of red fives inside the meld (0..=2 for chi/pon, 0..=1 for kan).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meld34 {
    pub kind: Meld34Kind,
    pub tiles: Vec<Tile34>,
    pub called_tile: Option<Tile34>,
    pub from_who: Option<u8>,
    pub aka_count: u8,
}

impl Meld34 {
    /// Number of physical tiles consumed by the meld (3 for chi/pon, 4 for kan).
    pub fn tile_count(&self) -> u8 {
        if self.kind.is_kan() {
            4
        } else {
            3
        }
    }

    /// Add this meld's tile contribution into a 34-count vector.
    /// Used to compose the *full* 34-counts for opponent-visibility tracking.
    pub fn add_to_counts(&self, counts: &mut Counts34) {
        match self.kind {
            Meld34Kind::Chi => {
                for t in &self.tiles {
                    counts[t.idx() as usize] += 1;
                }
            }
            Meld34Kind::Pon => {
                if let Some(t) = self.tiles.first() {
                    counts[t.idx() as usize] += 3;
                }
            }
            Meld34Kind::Daiminkan | Meld34Kind::Ankan | Meld34Kind::Kakan => {
                if let Some(t) = self.tiles.first() {
                    counts[t.idx() as usize] += 4;
                }
            }
        }
    }
}

/// Per-opponent observation used by the risk engine.
///
/// All discards are stored in turn order. `tedashi` parallels `discards`,
/// `true` for hand-cut (manual) and `false` for tsumogiri (auto-discard).
#[derive(Debug, Clone, Default)]
pub struct OpponentInfo {
    pub seat: u8,
    pub discards: Vec<Tile34>,
    pub tedashi: Vec<bool>,
    pub melds: Vec<Meld34>,
    pub is_riichi: bool,
    pub riichi_turn: Option<u8>,
    pub can_ippatsu: bool,
    pub jikaze: Tile34,
    /// Discards taken by other players' calls — we still treat them as
    /// "this player has seen / spat out this tile" for genbutsu purposes.
    pub called_from: Vec<Tile34>,
}

/// Closed-form input to the analysis engine.
///
/// Holds the active player's perspective: own hand, own melds, what we know
/// about each opponent, dora indicators, and the round/self winds. The engine
/// derives `left_tiles` (wall remainder) automatically when not provided.
#[derive(Debug, Clone)]
pub struct PlayerInfo34 {
    /// 3 (sanma) or 4 (yonma). Affects opponent count, tenpai-rate
    /// approximation, and seat-wind modulus.
    pub num_players: u8,
    /// Active seat (0..=3 for 4p, 0..=2 for 3p).
    pub seat: u8,
    /// Closed hand counts (excludes melded tiles). Length-of-hand obeys
    /// `13 - 3*open_meld_count` for 13-tile state, +1 for 14-tile state.
    pub hand: Counts34,
    /// Own melds (open + ankan). Tiles inside DO NOT appear in `hand`.
    pub melds: Vec<Meld34>,
    /// Number of red-fives we hold (in hand + melds).
    pub aka_count: u8,
    /// Dora-indicator tiles (the discards on the wall, not the active doras).
    pub dora_indicators: Vec<Tile34>,
    /// Round wind tile (`E`/`S`/`W`/`N`).
    pub bakaze: Tile34,
    /// Self wind tile.
    pub jikaze: Tile34,
    /// Turn count (own discards so far + own tsumo count). Used for risk weighting.
    pub turn: u8,
    /// Opponents — typically 3 entries for 4p, 2 for 3p.
    pub opponents: Vec<OpponentInfo>,
    /// Optional override for tiles still in the live wall + opponents' hands.
    /// If `None`, the analysis engine computes it as 4 - visible.
    pub left_tiles: Option<Counts34>,
    /// Own discards in order (used when this struct represents the post-tsumo
    /// state — the engine inspects the count to derive the turn).
    pub own_discards: Vec<Tile34>,
}

impl PlayerInfo34 {
    /// Total tiles in the closed hand.
    pub fn hand_size(&self) -> u8 {
        self.hand.iter().sum()
    }

    /// `len/3` parameter expected by `riichienv-core::shanten`.
    /// Matches `riichienv_core::shanten::calculate_shanten`: `hand_size / 3` (floor).
    /// For a normal 13-tile closed hand → 4. With one open meld (10 closed tiles) → 3.
    pub fn tehai_len_div3(&self) -> u8 {
        self.hand_size() / 3
    }

    /// Phase of the hand: true = 13-tile (waiting / ready to draw),
    /// false = 14-tile (ready to discard).
    pub fn is_drawing_state(&self) -> bool {
        self.hand_size() % 3 == 1
    }

    /// Build the wall + opponent-hand tile vector by subtracting every visible
    /// tile from a fresh 4-each pool. Used as default when `left_tiles` is None.
    pub fn compute_left_tiles(&self) -> Counts34 {
        if let Some(l) = self.left_tiles {
            return l;
        }
        let mut left = [4u8; TILE_COUNT];
        // Own hand
        for (l, h) in left.iter_mut().zip(self.hand.iter()) {
            *l = l.saturating_sub(*h);
        }
        // Own melds
        for m in &self.melds {
            let mut tmp = [0u8; TILE_COUNT];
            m.add_to_counts(&mut tmp);
            for (l, t) in left.iter_mut().zip(tmp.iter()) {
                *l = l.saturating_sub(*t);
            }
        }
        // Opponents — discards, melds, called-from-others (avoid double counting).
        for op in &self.opponents {
            for d in &op.discards {
                left[d.idx() as usize] = left[d.idx() as usize].saturating_sub(1);
            }
            for m in &op.melds {
                let mut tmp = [0u8; TILE_COUNT];
                m.add_to_counts(&mut tmp);
                // For chi/pon/kan the called tile came from a discard pile we
                // also count above. Subtract 1 for the called tile to avoid
                // double counting.
                if let Some(called) = m.called_tile {
                    tmp[called.idx() as usize] = tmp[called.idx() as usize].saturating_sub(1);
                }
                for (l, t) in left.iter_mut().zip(tmp.iter()) {
                    *l = l.saturating_sub(*t);
                }
            }
        }
        // Dora indicators are always visible.
        for d in &self.dora_indicators {
            left[d.idx() as usize] = left[d.idx() as usize].saturating_sub(1);
        }
        left
    }
}

/// Convenience builder used by tests and the snapshot adapter.
#[derive(Debug, Default)]
pub struct PlayerInfo34Builder {
    info: PlayerInfo34,
}

impl Default for PlayerInfo34 {
    fn default() -> Self {
        PlayerInfo34 {
            num_players: 4,
            seat: 0,
            hand: [0u8; TILE_COUNT],
            melds: Vec::new(),
            aka_count: 0,
            dora_indicators: Vec::new(),
            bakaze: Tile34(super::tile::HONOR_E),
            jikaze: Tile34(super::tile::HONOR_E),
            turn: 0,
            opponents: Vec::new(),
            left_tiles: None,
            own_discards: Vec::new(),
        }
    }
}

impl PlayerInfo34Builder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a tile by mjai string. Convenient for tests.
    pub fn tile(mut self, mjai: &str) -> Self {
        let t = Tile34::from_mjai(mjai).expect("valid mjai tile");
        self.info.hand[t.idx() as usize] += 1;
        if mjai.ends_with('r') {
            self.info.aka_count += 1;
        }
        self
    }

    pub fn add_many(mut self, mjai_list: &[&str]) -> Self {
        for s in mjai_list {
            self = self.tile(s);
        }
        self
    }

    pub fn meld(mut self, m: Meld34) -> Self {
        self.info.aka_count += m.aka_count;
        self.info.melds.push(m);
        self
    }

    pub fn bakaze(mut self, t: &str) -> Self {
        self.info.bakaze = Tile34::from_mjai(t).expect("valid wind");
        self
    }

    pub fn jikaze(mut self, t: &str) -> Self {
        self.info.jikaze = Tile34::from_mjai(t).expect("valid wind");
        self
    }

    pub fn build(self) -> PlayerInfo34 {
        self.info
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hand_size_and_phase() {
        let info = PlayerInfo34Builder::new()
            .add_many(&[
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p",
            ])
            .build();
        assert_eq!(info.hand_size(), 13);
        assert!(info.is_drawing_state());
        assert_eq!(info.tehai_len_div3(), 4); // floor(13/3) = 4
    }

    #[test]
    fn meld_counts_match() {
        let chi = Meld34 {
            kind: Meld34Kind::Chi,
            tiles: vec![
                Tile34::from_mjai("1m").unwrap(),
                Tile34::from_mjai("2m").unwrap(),
                Tile34::from_mjai("3m").unwrap(),
            ],
            called_tile: Some(Tile34::from_mjai("3m").unwrap()),
            from_who: Some(2),
            aka_count: 0,
        };
        let mut counts = [0u8; TILE_COUNT];
        chi.add_to_counts(&mut counts);
        assert_eq!(counts[0], 1); // 1m
        assert_eq!(counts[1], 1); // 2m
        assert_eq!(counts[2], 1); // 3m

        let pon = Meld34 {
            kind: Meld34Kind::Pon,
            tiles: vec![Tile34::from_mjai("E").unwrap()],
            called_tile: Some(Tile34::from_mjai("E").unwrap()),
            from_who: Some(1),
            aka_count: 0,
        };
        let mut c2 = [0u8; TILE_COUNT];
        pon.add_to_counts(&mut c2);
        assert_eq!(c2[27], 3);
    }

    #[test]
    fn left_tiles_subtracts_visibility() {
        let info = PlayerInfo34Builder::new().add_many(&["1m"; 4]).build();
        let left = info.compute_left_tiles();
        assert_eq!(left[0], 0); // we hold all four 1m
        assert_eq!(left[1], 4); // 2m untouched
    }
}
