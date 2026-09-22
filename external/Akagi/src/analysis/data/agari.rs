//! Agari-rate table (per-wait win probability lookup) and supporting
//! tile-type classification for the 27 number tiles.
//!
//! Source: the mahjong-helper Go tables `agari_rate_data.go` and
//! `risk_data.go`. Values are derived from
//! "勝つための現代麻雀技術論" / "「統計学」のマージャン戦術" (i.e. published
//! statistical estimates), used as factual numerical references.

/// Tile classification used by the agari-rate (and later, risk) lookup.
/// Mirrors the Go `tileType` enum exactly (same order = same discriminant).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TileTypeKind {
    NoSuji5 = 0,
    NoSuji46 = 1,
    NoSuji37 = 2,
    NoSuji28 = 3,
    NoSuji19 = 4,
    HalfSuji5 = 5,
    HalfSuji46A = 6,
    HalfSuji46B = 7,
    Suji37 = 8,
    Suji28 = 9,
    Suji19 = 10,
    DoubleSuji5 = 11,
    DoubleSuji46 = 12,
    YakuHaiLeft3 = 13,
    YakuHaiLeft2 = 14,
    YakuHaiLeft1 = 15,
    OtakazeLeft3 = 16,
    OtakazeLeft2 = 17,
    OtakazeLeft1 = 18,
}

impl TileTypeKind {
    pub fn from_index(i: u8) -> Option<Self> {
        match i {
            0 => Some(Self::NoSuji5),
            1 => Some(Self::NoSuji46),
            2 => Some(Self::NoSuji37),
            3 => Some(Self::NoSuji28),
            4 => Some(Self::NoSuji19),
            5 => Some(Self::HalfSuji5),
            6 => Some(Self::HalfSuji46A),
            7 => Some(Self::HalfSuji46B),
            8 => Some(Self::Suji37),
            9 => Some(Self::Suji28),
            10 => Some(Self::Suji19),
            11 => Some(Self::DoubleSuji5),
            12 => Some(Self::DoubleSuji46),
            _ => None,
        }
    }
}

/// `TileTypeTable[in-suit-position 0..9][safe-pattern]`
/// — used to classify a number tile based on which 4/5/6 are genbutsu.
/// Positions 1/9, 2/8, 3/7 take a single safe-flag; 4/6 use a 2-bit pattern.
pub const TILE_TYPE_TABLE: [&[TileTypeKind]; 9] = [
    &[TileTypeKind::NoSuji19, TileTypeKind::Suji19],
    &[TileTypeKind::NoSuji28, TileTypeKind::Suji28],
    &[TileTypeKind::NoSuji37, TileTypeKind::Suji37],
    &[
        TileTypeKind::NoSuji46,
        TileTypeKind::HalfSuji46B,
        TileTypeKind::HalfSuji46A,
        TileTypeKind::DoubleSuji46,
    ],
    &[
        TileTypeKind::NoSuji5,
        TileTypeKind::HalfSuji5,
        TileTypeKind::HalfSuji5,
        TileTypeKind::DoubleSuji5,
    ],
    &[
        TileTypeKind::NoSuji46,
        TileTypeKind::HalfSuji46A,
        TileTypeKind::HalfSuji46B,
        TileTypeKind::DoubleSuji46,
    ],
    &[TileTypeKind::NoSuji37, TileTypeKind::Suji37],
    &[TileTypeKind::NoSuji28, TileTypeKind::Suji28],
    &[TileTypeKind::NoSuji19, TileTypeKind::Suji19],
];

/// Number-tile agari rate, indexed by `[TileTypeKind][left 0..=4]`. Percent.
/// Rows are aligned with [`TileTypeKind::from_index`].
pub const AGARI_NUMBER: [[f64; 5]; 13] = [
    /* NoSuji5      */ [0.0, 11.8, 20.3, 26.7, 31.0],
    /* NoSuji46     */ [0.0, 11.8, 20.3, 26.7, 31.0],
    /* NoSuji37     */ [0.0, 14.8, 25.5, 32.0, 36.8],
    /* NoSuji28     */ [0.0, 19.2, 31.7, 38.2, 42.0],
    /* NoSuji19     */ [0.0, 26.3, 41.6, 50.1, 54.0],
    /* HalfSuji5    */ [0.0, 12.9, 24.7, 30.9, 35.4],
    /* HalfSuji46A  */ [0.0, 12.9, 24.7, 30.9, 35.4],
    /* HalfSuji46B  */ [0.0, 12.9, 24.7, 30.9, 35.4],
    /* Suji37       */ [0.0, 17.2, 33.1, 43.5, 48.9],
    /* Suji28       */ [0.0, 24.9, 42.7, 51.2, 56.5],
    /* Suji19       */ [0.0, 36.1, 60.0, 67.9, 0.0],
    /* DoubleSuji5  */ [0.0, 16.5, 35.5, 45.4, 50.0],
    /* DoubleSuji46 */ [0.0, 16.5, 35.5, 45.4, 50.0],
];

/// Honor-tile agari rate when the wait is non-tanki (i.e. shanpon / paired).
/// Indexed by `[left 0..=2]`. Percent.
pub const AGARI_HONOR_NON_DANKI: [f64; 3] = [0.0, 34.0, 52.0];

/// Honor-tile agari rate when the wait is tanki (single-wait pair head).
/// Indexed by `[left 0..=3]`. Percent. Anchored at turn 8 in the Go source.
pub const AGARI_HONOR_DANKI: [f64; 4] = [0.0, 47.5, 58.0, 49.5];

/// Base agari rate per draw when the hand is in furiten state.
pub const FURITEN_BASE: f64 = 5.9;

/// Multiplier applied when a wait coincides with a dora honor tile.
pub const HONOR_DORA_MULTI: f64 = 35.0 / 48.0;

/// Multiplier applied when a wait coincides with a dora number tile.
pub const NUMBER_DORA_MULTI: f64 = 26.0 / 38.0;

/// Multiplier applied to the overall avg-agari-rate when waits form a
/// ryanmen / sanmen-machi shape (proxy: same suit, 3-tile-spaced sequence).
pub const RYANMEN_MULTI: f64 = 0.91;

/// Classify number tiles 0..27 into a tileType, given an array of 34 booleans
/// describing which tiles have already shown up as our own discards.
///
/// Mirrors `calcTileType27` from the mahjong-helper Go `risk_base.go`.
pub fn classify_tile_type_27(safe_tiles_34: &[bool; 34]) -> [TileTypeKind; 27] {
    let mut out = [TileTypeKind::NoSuji5; 27];
    let safe = |i: usize| if safe_tiles_34[i] { 1usize } else { 0usize };
    for suit in 0..3 {
        for (j, row) in TILE_TYPE_TABLE.iter().enumerate().take(3) {
            let idx = 9 * suit + j;
            out[idx] = row[safe(idx + 3)];
        }
        for (j, row) in TILE_TYPE_TABLE.iter().enumerate().skip(3).take(3) {
            let idx = 9 * suit + j;
            let pattern = (safe(idx - 3) << 1) | safe(idx + 3);
            out[idx] = row[pattern];
        }
        for (j, row) in TILE_TYPE_TABLE.iter().enumerate().skip(6).take(3) {
            let idx = 9 * suit + j;
            out[idx] = row[safe(idx - 3)];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_no_safe_tiles_gives_no_suji() {
        let safe = [false; 34];
        let cls = classify_tile_type_27(&safe);
        // Every position should be NoSuji* with no safety hints.
        assert_eq!(cls[0], TileTypeKind::NoSuji19); // 1m
        assert_eq!(cls[4], TileTypeKind::NoSuji5); // 5m
        assert_eq!(cls[8], TileTypeKind::NoSuji19); // 9m
        assert_eq!(cls[9 + 3], TileTypeKind::NoSuji46); // 4p
    }

    #[test]
    fn safe_5_makes_suji() {
        let mut safe = [false; 34];
        safe[4] = true; // 5m discarded
        let cls = classify_tile_type_27(&safe);
        assert_eq!(cls[1], TileTypeKind::Suji28); // 2m → suji
        assert_eq!(cls[7], TileTypeKind::Suji28); // 8m → suji
    }

    #[test]
    fn double_suji_5_when_2_and_8_safe() {
        let mut safe = [false; 34];
        safe[1] = true; // 2m
        safe[7] = true; // 8m
        let cls = classify_tile_type_27(&safe);
        assert_eq!(cls[4], TileTypeKind::DoubleSuji5);
    }

    #[test]
    fn agari_table_structure() {
        // No-suji 19 with 4 left = 54.0% (peak)
        assert_eq!(AGARI_NUMBER[TileTypeKind::NoSuji19 as usize][4], 54.0);
        // Suji 1/9 with 4 left = 0 (impossible — used for symmetry)
        assert_eq!(AGARI_NUMBER[TileTypeKind::Suji19 as usize][4], 0.0);
    }
}
