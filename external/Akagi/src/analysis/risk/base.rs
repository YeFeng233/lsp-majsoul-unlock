//! Per-tile deal-in (放铳) risk against a single opponent.
//!
//! Source: the mahjong-helper Go `risk_base.go`. Returns a `[f64; 34]`
//! risk vector that downstream callers can adjust further with the `fix_*`
//! helpers (early-outside, point, dora, global multiplier).

use super::wall;
use crate::analysis::data::agari::{TileTypeKind, TILE_TYPE_TABLE};
use crate::analysis::data::risk::{FIXED_DORA_RISK_MULTI, HONOR_TILE_TYPE, MAX_TURN, RISK_RATE};
use crate::analysis::hand::Counts34;
use crate::analysis::tile::{Tile34, HONOR_BASE, HONOR_C, TILE_COUNT};

pub type RiskVec = [f64; TILE_COUNT];

/// Mark suit positions whose removal makes neighbouring tiles "low-risk"
/// regardless of suji status. Returns 27 booleans (one per suit tile),
/// `true` = low risk against ryanmen.
fn calc_low_risk_27(safe_tiles_34: &[bool; TILE_COUNT], left_tiles: &Counts34) -> [bool; 27] {
    let mut low = [false; 27];
    for (i, b) in low.iter_mut().enumerate().take(27) {
        if safe_tiles_34[i] {
            *b = true;
        }
    }
    for suit in 0..3usize {
        // 2-extinct → treat 1 as discarded
        if left_tiles[9 * suit + 1] == 0 {
            low[9 * suit] = true;
        }
        // 3-extinct → treat 12 as discarded
        if left_tiles[9 * suit + 2] == 0 {
            low[9 * suit] = true;
            low[9 * suit + 1] = true;
        }
        // 4-extinct → treat 23 as discarded
        if left_tiles[9 * suit + 3] == 0 {
            low[9 * suit + 1] = true;
            low[9 * suit + 2] = true;
        }
        // 6-extinct → treat 78 as discarded
        if left_tiles[9 * suit + 5] == 0 {
            low[9 * suit + 6] = true;
            low[9 * suit + 7] = true;
        }
        // 7-extinct → treat 89 as discarded
        if left_tiles[9 * suit + 6] == 0 {
            low[9 * suit + 7] = true;
            low[9 * suit + 8] = true;
        }
        // 8-extinct → treat 9 as discarded
        if left_tiles[9 * suit + 7] == 0 {
            low[9 * suit + 8] = true;
        }
    }
    low
}

#[inline]
fn idx_of_kind(k: TileTypeKind) -> usize {
    k as usize
}

#[inline]
fn risk_for(turns: usize, k: TileTypeKind) -> f64 {
    let t = turns.clamp(1, MAX_TURN);
    RISK_RATE[t][idx_of_kind(k)]
}

/// Compute base 34-tile risk vector for one opponent. Mirrors Go's
/// `CalculateRiskTiles34`.
///
/// `turns` = opponent's discard count (their "round").
/// `safe_tiles_34` = our knowledge of what they cannot win on (genbutsu +
/// post-riichi pass-throughs + others' discards in their riichi window).
/// `left_tiles` = wall + opponents' hands (raw remaining count, 0..=4).
/// `dora_tiles` = active dora tiles (not the indicators).
/// `bakaze_tile` / `jikaze_tile` = round wind / opponent's seat wind tile in 0..34.
pub fn risk_tiles(
    turns: usize,
    safe_tiles_34: &[bool; TILE_COUNT],
    left_tiles: &Counts34,
    dora_tiles: &[Tile34],
    bakaze_tile: Tile34,
    jikaze_tile: Tile34,
) -> RiskVec {
    let mut risk = [0.0f64; TILE_COUNT];

    let dora_multi = |tile: u8, kind: TileTypeKind| -> f64 {
        let mut m = 1.0;
        for d in dora_tiles {
            if d.idx() == tile {
                m *= FIXED_DORA_RISK_MULTI[idx_of_kind(kind)];
            }
        }
        m
    };

    let low = calc_low_risk_27(safe_tiles_34, left_tiles);

    // Number tiles by suit position (matches Go layout exactly).
    for suit in 0..3usize {
        for (j, row) in TILE_TYPE_TABLE.iter().enumerate().take(3) {
            let idx = 9 * suit + j;
            let pat = if low[idx + 3] { 1usize } else { 0 };
            let k = row[pat];
            risk[idx] = risk_for(turns, k) * dora_multi(idx as u8, k);
            // Suit-1 + paired neighbour discarded + 1 itself extinct → genbutsu shape.
            if j == 0 && safe_tiles_34[idx + 3] && left_tiles[idx] == 0 {
                risk[idx] = 0.0;
            }
        }
        for (j, row) in TILE_TYPE_TABLE.iter().enumerate().skip(3).take(3) {
            let idx = 9 * suit + j;
            let lo = if low[idx - 3] { 1usize } else { 0 };
            let hi = if low[idx + 3] { 1usize } else { 0 };
            let pat = (lo << 1) | hi;
            let k = row[pat];
            risk[idx] = risk_for(turns, k) * dora_multi(idx as u8, k);
        }
        for (j, row) in TILE_TYPE_TABLE.iter().enumerate().skip(6).take(3) {
            let idx = 9 * suit + j;
            let pat = if low[idx - 3] { 1usize } else { 0 };
            let k = row[pat];
            risk[idx] = risk_for(turns, k) * dora_multi(idx as u8, k);
            if j == 8 && safe_tiles_34[idx - 3] && left_tiles[idx] == 0 {
                risk[idx] = 0.0;
            }
        }
        // 5 extinct → 3 / 7 act like suji-3/7.
        if left_tiles[9 * suit + 4] == 0 {
            let k = TileTypeKind::Suji37;
            risk[9 * suit + 2] = risk_for(turns, k) * dora_multi((9 * suit + 2) as u8, k);
            risk[9 * suit + 6] = risk_for(turns, k) * dora_multi((9 * suit + 6) as u8, k);
        }
    }

    // Honor tiles.
    for i in HONOR_BASE as usize..=HONOR_C as usize {
        let left_count = left_tiles[i];
        if left_count == 0 {
            risk[i] = 0.0;
            continue;
        }
        let is_yakuhai =
            (i as u8) == bakaze_tile.idx() || (i as u8) == jikaze_tile.idx() || (i as u8) >= 31; // P / F / C (always yakuhai)
        let row_idx = if is_yakuhai { 1 } else { 0 };
        let col_idx = (left_count.saturating_sub(1) as usize).min(3);
        let k = HONOR_TILE_TYPE[row_idx][col_idx];
        risk[i] = risk_for(turns, k) * dora_multi(i as u8, k);
    }

    // No-chance overrides (from wall analysis).
    for entry in wall::no_chance(left_tiles) {
        let idx = entry.tile34 as usize;
        let pos = (idx % 9) + 1; // 1..=9
        let (k, mul) = match pos {
            1 | 9 => (TileTypeKind::Suji19, 1.0),
            2 | 8 => (TileTypeKind::Suji19, 1.1),
            3 | 7 => (TileTypeKind::Suji28, 1.0),
            4 | 6 => (TileTypeKind::DoubleSuji46, 1.0),
            5 => (TileTypeKind::DoubleSuji5, 1.0),
            _ => continue,
        };
        risk[idx] = risk_for(turns, k) * mul * dora_multi(idx as u8, k);
    }

    // Double-no-chance overrides.
    for entry in wall::double_no_chance_with_discards(left_tiles, safe_tiles_34) {
        let idx = entry.tile34 as usize;
        if left_tiles[idx] == 0 {
            risk[idx] = 0.0;
            continue;
        }
        let k = TileTypeKind::Suji19;
        risk[idx] = risk_for(turns, k) * dora_multi(idx as u8, k);
        let pos = idx % 9;
        if pos > 0 && pos < 8 {
            risk[idx] *= 1.1;
        }
    }

    // Genbutsu always wins.
    for (i, is_safe) in safe_tiles_34.iter().enumerate() {
        if *is_safe {
            risk[i] = 0.0;
        }
    }

    risk
}

/// Apply the early-outside (1-5巡 外側牌) correction: tiles already on the
/// player's river get their risk multiplied by 0.4.
pub fn fix_with_early_outside(risk: &mut RiskVec, early_outside: &[Tile34]) {
    for t in early_outside {
        risk[t.idx() as usize] *= 0.4;
    }
}

/// Multiply every entry by a global factor (used by tenpai-rate / point fixes).
pub fn fix_with_global(risk: &mut RiskVec, multi: f64) {
    for r in risk.iter_mut() {
        *r *= multi;
    }
}

/// Adjust risk by the opponent's expected ron point relative to the
/// `RonPointRiichi` baseline. Higher expected payouts → higher risk.
pub fn fix_with_point(risk: &mut RiskVec, ron_point: f64) {
    use crate::analysis::data::point::RON_POINT_RIICHI;
    fix_with_global(risk, ron_point / RON_POINT_RIICHI);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_left() -> Counts34 {
        [4u8; TILE_COUNT]
    }

    #[test]
    fn risk_zero_for_genbutsu() {
        let mut safe = [false; TILE_COUNT];
        safe[10] = true; // 2p genbutsu
        let risk = risk_tiles(
            9,
            &safe,
            &full_left(),
            &[],
            Tile34(crate::analysis::tile::HONOR_E),
            Tile34(crate::analysis::tile::HONOR_E),
        );
        assert_eq!(risk[10], 0.0);
    }

    #[test]
    fn no_suji_5_anchor_at_turn_9() {
        let safe = [false; TILE_COUNT];
        let risk = risk_tiles(
            9,
            &safe,
            &full_left(),
            &[],
            Tile34(crate::analysis::tile::HONOR_E),
            Tile34(crate::analysis::tile::HONOR_E),
        );
        // 5m at turn 9 with no info should sit at the NoSuji5 baseline (12.8).
        assert!((risk[4] - 12.8).abs() < 0.01);
    }

    #[test]
    fn yakuhai_more_dangerous_than_otakaze() {
        let safe = [false; TILE_COUNT];
        let risk = risk_tiles(
            9,
            &safe,
            &full_left(),
            &[],
            Tile34(crate::analysis::tile::HONOR_E),
            Tile34(crate::analysis::tile::HONOR_E),
        );
        // P (idx 31, dragon = always yakuhai) should be > S (idx 28, otakaze when E is round/seat).
        assert!(risk[31] > risk[28]);
    }

    #[test]
    fn extinct_5_makes_37_safer() {
        let mut left = full_left();
        left[4] = 0; // 5m extinct
        let safe = [false; TILE_COUNT];
        let risk = risk_tiles(
            9,
            &safe,
            &left,
            &[],
            Tile34(crate::analysis::tile::HONOR_E),
            Tile34(crate::analysis::tile::HONOR_E),
        );
        // 5-extinct first sets 3/7 to Suji37 baseline (5.5), then the NC pass
        // overrides them to Suji28 (3.9 at turn 9). Both should be safer than
        // the no-information NoSuji37 baseline (9.5 at turn 9).
        assert!(risk[2] < 9.5, "3m={}", risk[2]);
        assert!(risk[6] < 9.5, "7m={}", risk[6]);
        // And both must end up at the NC override = Suji28 = 3.9.
        assert!((risk[2] - 3.9).abs() < 0.1, "3m={}", risk[2]);
        assert!((risk[6] - 3.9).abs() < 0.1, "7m={}", risk[6]);
    }

    #[test]
    fn dora_inflates_risk() {
        let safe = [false; TILE_COUNT];
        let dora = [Tile34::from_mjai("5m").unwrap()];
        let with_dora = risk_tiles(
            9,
            &safe,
            &full_left(),
            &dora,
            Tile34(crate::analysis::tile::HONOR_E),
            Tile34(crate::analysis::tile::HONOR_E),
        );
        let no_dora = risk_tiles(
            9,
            &safe,
            &full_left(),
            &[],
            Tile34(crate::analysis::tile::HONOR_E),
            Tile34(crate::analysis::tile::HONOR_E),
        );
        assert!(with_dora[4] > no_dora[4]);
    }

    #[test]
    fn early_outside_halves_to_40_pct() {
        let safe = [false; TILE_COUNT];
        let mut risk = risk_tiles(
            9,
            &safe,
            &full_left(),
            &[],
            Tile34(crate::analysis::tile::HONOR_E),
            Tile34(crate::analysis::tile::HONOR_E),
        );
        let before = risk[0];
        fix_with_early_outside(&mut risk, &[Tile34::from_mjai("1m").unwrap()]);
        assert!((risk[0] - before * 0.4).abs() < 1e-9);
    }
}
