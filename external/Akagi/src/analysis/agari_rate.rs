//! Per-wait agari rate.
//!
//! Mirrors the mahjong-helper `agari_rate.go` table. Returns a percentage
//! probability for each wait tile, given:
//!   - the player's own discards (used to classify each tile's suji status),
//!   - the dora indicator-derived dora set,
//!   - whether the hand is in furiten state.
//!
//! The single-wait honor (tanki) case has its own table.

use std::collections::BTreeMap;

use super::data::agari::{
    classify_tile_type_27, AGARI_HONOR_DANKI, AGARI_HONOR_NON_DANKI, AGARI_NUMBER, FURITEN_BASE,
    HONOR_DORA_MULTI, NUMBER_DORA_MULTI, RYANMEN_MULTI,
};
use super::tile::Tile34;
use super::waits::Waits;

/// Per-tile agari rate (percent).
pub type TileAgariRate = BTreeMap<u8, f64>;

/// Classify suit tiles based on the player's own discard list.
fn safe_tiles_from_discards(own_discards: &[Tile34]) -> [bool; 34] {
    let mut safe = [false; 34];
    for d in own_discards {
        safe[d.idx() as usize] = true;
    }
    safe
}

fn is_dora(tile: u8, dora: &[Tile34]) -> bool {
    dora.iter().any(|d| d.idx() == tile)
}

/// Compose two independent probabilities `a + b - a*b/100` (operating in percent).
pub fn compose(a: f64, b: f64) -> f64 {
    a + b - a * b / 100.0
}

/// Per-wait agari rate. `own_discards` is the player's discard pile (used to
/// classify suji); `dora_tiles` are the active dora tiles (not the indicators).
pub fn per_wait(
    waits: &Waits,
    own_discards: &[Tile34],
    dora_tiles: &[Tile34],
    is_furiten: bool,
) -> TileAgariRate {
    let mut out: TileAgariRate = BTreeMap::new();

    if is_furiten {
        for (tile, left) in waits.iter() {
            let mut rate = 0.0;
            for _ in 0..left {
                rate = compose(rate, FURITEN_BASE);
            }
            out.insert(tile, rate);
        }
        return out;
    }

    // Single-wait honor (tanki): special table.
    if waits.len() == 1 {
        if let Some((tile, left)) = waits.iter().next() {
            if tile >= 27 {
                let idx = std::cmp::min(left as usize, AGARI_HONOR_DANKI.len() - 1);
                let mut rate = AGARI_HONOR_DANKI[idx];
                if is_dora(tile, dora_tiles) {
                    rate *= HONOR_DORA_MULTI;
                }
                out.insert(tile, rate);
                return out;
            }
        }
    }

    let safe = safe_tiles_from_discards(own_discards);
    let cls27 = classify_tile_type_27(&safe);

    for (tile, left) in waits.iter() {
        let left_idx = std::cmp::min(left as usize, 4);
        let mut rate = if tile < 27 {
            let kind = cls27[tile as usize];
            // Number-tile table only covers the 13 number-tile classes.
            // Yakuhai/Otakaze classes are honor classifications; we shouldn't
            // hit them here because `tile < 27` selects number tiles only.
            let row = kind as usize;
            AGARI_NUMBER[row][left_idx]
        } else {
            let h_idx = std::cmp::min(left as usize, AGARI_HONOR_NON_DANKI.len() - 1);
            AGARI_HONOR_NON_DANKI[h_idx]
        };
        if is_dora(tile, dora_tiles) {
            rate *= if tile >= 27 {
                HONOR_DORA_MULTI
            } else {
                NUMBER_DORA_MULTI
            };
        }
        out.insert(tile, rate);
    }

    out
}

/// Average overall agari rate across all waits, including the ryanmen
/// correction when the wait shape resembles a 2-tile or 3-tile suji ladder.
pub fn average(
    waits: &Waits,
    own_discards: &[Tile34],
    dora_tiles: &[Tile34],
    is_furiten: bool,
) -> f64 {
    if is_furiten {
        let mut rate = 0.0;
        let total = waits.total_left();
        for _ in 0..total {
            rate = compose(rate, FURITEN_BASE);
        }
        return rate;
    }

    let per = per_wait(waits, own_discards, dora_tiles, is_furiten);
    let mut agg = 0.0;
    for r in per.values() {
        agg = compose(agg, *r);
    }

    // Ryanmen / sanmen-machi adjust: same-suit waits separated by 3.
    let mut wait_tiles: Vec<u8> = waits
        .iter()
        .filter(|(_, l)| *l > 0)
        .map(|(t, _)| t)
        .collect();
    if wait_tiles.len() > 1 {
        if wait_tiles.iter().any(|&t| t >= 27) {
            return agg;
        }
        let suit = wait_tiles[0] / 9;
        if !wait_tiles.iter().all(|&t| t / 9 == suit) {
            return agg;
        }
        wait_tiles.sort();
        let is_ryanmen = wait_tiles.len() == 2 && wait_tiles[0] + 3 == wait_tiles[1];
        let is_sanmen = wait_tiles.len() == 3
            && wait_tiles[0] + 3 == wait_tiles[1]
            && wait_tiles[1] + 3 == wait_tiles[2];
        if is_ryanmen || is_sanmen {
            agg *= RYANMEN_MULTI;
        }
    }

    agg
}

/// Whether the player is in furiten — any of the wait tiles already appears
/// in their own discard pile.
pub fn is_furiten(waits: &Waits, own_discards: &[Tile34]) -> bool {
    let mut waited = [false; 34];
    for (t, _) in waits.iter() {
        waited[t as usize] = true;
    }
    own_discards.iter().any(|d| waited[d.idx() as usize])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::tile::Tile34;
    use crate::analysis::waits::Waits;

    fn waits_of(pairs: &[(&str, u8)]) -> Waits {
        let mut w = Waits::new();
        for (m, c) in pairs {
            w.insert(Tile34::from_mjai(m).unwrap().idx(), *c);
        }
        w
    }

    #[test]
    fn furiten_compounds() {
        // 2 copies of a wait tile in furiten → ~5.9 + 5.9 - 5.9*5.9/100 ≈ 11.45%.
        let w = waits_of(&[("3p", 2)]);
        let rate = average(&w, &[Tile34::from_mjai("3p").unwrap()], &[], true);
        let expected = compose(FURITEN_BASE, FURITEN_BASE);
        assert!((rate - expected).abs() < 1e-6);
    }

    #[test]
    fn ryanmen_correction_applied() {
        // 4-7p ryanmen, 4 left each, no discards.
        let w = waits_of(&[("4p", 4), ("7p", 4)]);
        let raw = {
            let per = per_wait(&w, &[], &[], false);
            let mut agg = 0.0;
            for r in per.values() {
                agg = compose(agg, *r);
            }
            agg
        };
        let avg = average(&w, &[], &[], false);
        assert!((avg - raw * RYANMEN_MULTI).abs() < 1e-6);
    }

    #[test]
    fn dora_boosts_honor_tanki() {
        let w = waits_of(&[("E", 3)]);
        let dora = [Tile34::from_mjai("E").unwrap()];
        let plain = per_wait(&w, &[], &[], false);
        let with_dora = per_wait(&w, &[], &dora, false);
        let plain_e = plain[&Tile34::from_mjai("E").unwrap().idx()];
        let dora_e = with_dora[&Tile34::from_mjai("E").unwrap().idx()];
        assert!((dora_e - plain_e * HONOR_DORA_MULTI).abs() < 1e-6);
    }

    #[test]
    fn furiten_detection() {
        let w = waits_of(&[("3p", 4)]);
        assert!(is_furiten(&w, &[Tile34::from_mjai("3p").unwrap()]));
        assert!(!is_furiten(&w, &[Tile34::from_mjai("4p").unwrap()]));
    }
}
