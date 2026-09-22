//! Deal-in (放铳) risk tables.
//!
//! Source: the mahjong-helper Go `risk_data.go` table. The tile-type
//! enum and the suit-only `TILE_TYPE_TABLE` already live in
//! [`super::agari`]; this module adds:
//!   - `RISK_RATE[turn][type]` — the percentage deal-in rate per (round, type).
//!   - `FIXED_DORA_RISK_MULTI[type]` — combined deal-in + lost-points multiplier
//!     applied when the candidate tile is dora.
//!   - `HONOR_TILE_TYPE[is_yakuhai][left-1]` — honor-tile classification.
//!   - Per-opponent `RonPoint` constants are re-exported from `data::point`.

use super::agari::TileTypeKind;

/// Number of indexed turns in the risk table (1..=18 produced by the source;
/// index 0 is empty / index 19 is a small fallback). For an opponent on turn
/// `t`, look up `RISK_RATE[clamp(t, 1, MAX_TURN)]`.
pub const MAX_TURN: usize = 19;

/// `RISK_RATE[turn][type] = deal-in rate (%)` indexed by `TileTypeKind` ordinal.
/// Row 0 is empty (turn 0 doesn't occur). Row 19 is a hyperbolic upper bound
/// for late-game extrapolation.
pub const RISK_RATE: [[f64; 19]; 20] = [
    // turn 0 — unused, all zeros so accidental indexing is safe.
    [0.0; 19],
    // turn 1
    [
        5.7, 5.7, 5.8, 4.7, 3.4, 2.5, 2.5, 3.1, 5.6, 3.8, 1.8, 0.8, 2.6, 2.1, 1.2, 0.5, 2.4, 1.4,
        1.2,
    ],
    // turn 2
    [
        6.6, 6.9, 6.3, 5.2, 4.0, 3.5, 3.5, 4.1, 5.3, 3.5, 1.9, 0.8, 2.6, 2.3, 1.2, 0.5, 2.7, 1.3,
        0.4,
    ],
    // turn 3
    [
        7.7, 8.0, 6.7, 5.8, 4.6, 4.3, 4.1, 4.9, 5.2, 3.6, 1.8, 1.6, 2.0, 2.4, 1.2, 0.3, 2.6, 1.2,
        0.3,
    ],
    // turn 4
    [
        8.5, 8.9, 7.1, 6.2, 5.1, 4.8, 4.7, 5.6, 5.2, 3.8, 1.7, 1.6, 2.0, 2.6, 1.1, 0.2, 2.6, 1.2,
        0.2,
    ],
    // turn 5
    [
        9.4, 9.7, 7.5, 6.7, 5.5, 5.3, 5.1, 6.0, 5.3, 3.7, 1.7, 1.7, 2.0, 2.9, 1.2, 0.2, 2.8, 1.2,
        0.2,
    ],
    // turn 6
    [
        10.2, 10.5, 7.9, 7.1, 5.9, 5.8, 5.6, 6.4, 5.2, 3.7, 1.7, 1.8, 2.0, 3.2, 1.3, 0.2, 2.9, 1.3,
        0.2,
    ],
    // turn 7
    [
        11.0, 11.3, 8.4, 7.5, 6.3, 6.3, 6.1, 6.8, 5.3, 3.7, 1.7, 2.0, 2.1, 3.6, 1.4, 0.2, 3.2, 1.4,
        0.2,
    ],
    // turn 8
    [
        11.9, 12.2, 8.9, 8.0, 6.8, 6.9, 6.6, 7.4, 5.3, 3.8, 1.7, 2.1, 2.2, 4.0, 1.6, 0.2, 3.5, 1.6,
        0.2,
    ],
    // turn 9
    [
        12.8, 13.1, 9.5, 8.6, 7.4, 7.4, 7.2, 7.9, 5.5, 3.9, 1.8, 2.2, 2.3, 4.6, 1.9, 0.3, 4.0, 1.8,
        0.2,
    ],
    // turn 10
    [
        13.8, 14.1, 10.1, 9.2, 8.0, 8.0, 7.8, 8.5, 5.6, 4.0, 1.9, 2.4, 2.4, 5.3, 2.2, 0.3, 4.6,
        2.1, 0.3,
    ],
    // turn 11
    [
        14.9, 15.1, 10.8, 9.9, 8.7, 8.7, 8.5, 9.2, 5.7, 4.2, 2.0, 2.5, 2.6, 6.0, 2.6, 0.4, 5.1,
        2.5, 0.3,
    ],
    // turn 12
    [
        16.0, 16.3, 11.6, 10.6, 9.4, 9.4, 9.2, 9.9, 6.0, 4.4, 2.2, 2.7, 2.7, 6.8, 3.1, 0.4, 5.1,
        2.5, 0.3,
    ],
    // turn 13
    [
        17.2, 17.5, 12.4, 11.4, 10.2, 10.2, 10.0, 10.6, 6.2, 4.6, 2.4, 3.0, 3.0, 7.8, 3.7, 0.5,
        6.6, 3.7, 0.5,
    ],
    // turn 14
    [
        18.5, 18.8, 13.3, 12.3, 11.1, 11.0, 10.9, 11.4, 6.6, 4.9, 2.7, 3.2, 3.1, 8.8, 4.4, 0.7,
        7.4, 4.4, 0.6,
    ],
    // turn 15
    [
        19.9, 20.1, 14.3, 13.3, 12.0, 11.9, 11.8, 12.3, 7.0, 5.3, 3.0, 3.4, 3.4, 9.9, 5.2, 0.8,
        8.4, 5.3, 0.8,
    ],
    // turn 16
    [
        21.3, 21.7, 15.4, 14.3, 13.1, 12.9, 12.8, 13.3, 7.4, 5.7, 3.3, 3.7, 3.6, 11.2, 6.2, 1.0,
        9.4, 6.5, 0.9,
    ],
    // turn 17
    [
        22.9, 23.2, 16.6, 15.4, 14.2, 14.0, 13.8, 14.4, 8.0, 6.1, 3.6, 3.9, 3.9, 12.4, 7.3, 1.3,
        10.5, 7.7, 1.2,
    ],
    // turn 18
    [
        24.7, 24.9, 17.9, 16.7, 15.4, 15.2, 15.0, 15.6, 8.5, 6.6, 4.0, 4.3, 4.2, 13.9, 8.5, 1.7,
        11.8, 9.4, 1.6,
    ],
    // turn 19 (overflow — small extrapolation)
    [
        27.5, 27.8, 20.4, 19.1, 17.8, 17.5, 17.5, 17.5, 9.8, 7.4, 5.0, 5.1, 5.1, 18.1, 12.1, 2.8,
        14.7, 12.6, 2.1,
    ],
];

/// Multiplier applied when the tile is dora — combines deal-in rate increase
/// AND average-lost-points increase. Indexed by `TileTypeKind` ordinal.
pub const FIXED_DORA_RISK_MULTI: [f64; 19] = [
    14.9 / 12.8 * 78.0 / 58.0,
    15.0 / 13.1 * 78.0 / 58.0,
    12.1 / 9.5 * 75.0 / 56.0,
    10.3 / 8.6 * 75.0 / 54.0,
    8.9 / 7.4 * 77.0 / 53.0,
    9.7 / 7.4 * 81.0 / 60.0,
    8.9 / 7.2 * 81.0 / 60.0,
    10.4 / 7.9 * 81.0 / 60.0,
    8.0 / 5.5 * 75.0 / 56.0,
    5.5 / 3.9 * 81.0 / 56.0,
    3.5 / 1.8 * 92.0 / 58.0,
    4.1 / 2.2 * 88.0 / 62.0,
    4.1 / 2.3 * 88.0 / 62.0,
    5.2 / 4.6 * 96.0 / 67.0,
    2.9 / 1.9 * 96.0 / 67.0,
    1.1 / 0.3 * 96.0 / 67.0,
    5.1 / 4.0 * 92.0 / 56.0,
    3.0 / 1.8 * 92.0 / 56.0,
    0.8 / 0.2 * 92.0 / 56.0,
];

/// Honor-tile classification.
/// `HONOR_TILE_TYPE[is_yakuhai 0..=1][remaining-1 0..=3]`.
pub const HONOR_TILE_TYPE: [[TileTypeKind; 4]; 2] = [
    // Otakaze (non-yakuhai winds)
    [
        TileTypeKind::OtakazeLeft1,
        TileTypeKind::OtakazeLeft2,
        TileTypeKind::OtakazeLeft3,
        TileTypeKind::OtakazeLeft3, // 4 left → still treated as Left3
    ],
    // Yakuhai (round wind / seat wind / dragon)
    [
        TileTypeKind::YakuHaiLeft1,
        TileTypeKind::YakuHaiLeft2,
        TileTypeKind::YakuHaiLeft3,
        TileTypeKind::YakuHaiLeft3,
    ],
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risk_rate_anchor_at_turn_9() {
        // From the Go reference, NoSuji5 at turn 9 = 12.8.
        assert_eq!(RISK_RATE[9][TileTypeKind::NoSuji5 as usize], 12.8);
        // Suji 1/9 at turn 9 = 1.8.
        assert_eq!(RISK_RATE[9][TileTypeKind::Suji19 as usize], 1.8);
    }

    #[test]
    fn dora_multi_increases_for_no_suji_5() {
        // Dora multi for the most-frequently-targeted no-suji 5 should be > 1.
        let m = FIXED_DORA_RISK_MULTI[TileTypeKind::NoSuji5 as usize];
        assert!(m > 1.0);
    }

    #[test]
    fn honor_classification_yakuhai_vs_otakaze() {
        // 3 left, yakuhai → YakuHaiLeft3.
        assert_eq!(HONOR_TILE_TYPE[1][2], TileTypeKind::YakuHaiLeft3);
        // 1 left, otakaze → OtakazeLeft1.
        assert_eq!(HONOR_TILE_TYPE[0][0], TileTypeKind::OtakazeLeft1);
    }
}
