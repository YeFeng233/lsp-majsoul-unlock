//! Per-tile wait enumeration.
//!
//! Works at any shanten level: for each of the 34 tile types, simulate
//! drawing it and recompute shanten. If the new shanten is strictly less
//! than the current shanten, the tile is a *progressing* draw (a wait at
//! tenpai, an effective tile elsewhere).
//!
//! Mirrors mahjong-helper's `Waits` map plus the per-tile remaining-count
//! tracking used by the agari-rate engine.

use std::collections::BTreeMap;

use serde::Serialize;

use super::hand::{Counts34, PlayerInfo34};
use super::shanten::{self, Shanten};
use super::tile::{Tile34, TILE_COUNT};

/// Map from `Tile34` index → count of that tile still in the wall + opponents.
/// Renders deterministically because it's a `BTreeMap`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Waits {
    pub map: BTreeMap<u8, u8>,
}

impl Waits {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, tile: u8, left: u8) {
        if left > 0 {
            self.map.insert(tile, left);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Sum of remaining tiles across all waits.
    pub fn total_left(&self) -> u32 {
        self.map.values().map(|&v| v as u32).sum()
    }

    /// Iterator over (tile, left).
    pub fn iter(&self) -> impl Iterator<Item = (u8, u8)> + '_ {
        self.map.iter().map(|(&t, &c)| (t, c))
    }

    /// Render as mjai-string list (sorted).
    pub fn to_mjai(&self) -> Vec<(String, u8)> {
        self.map
            .iter()
            .map(|(&t, &c)| (Tile34(t).to_mjai().to_string(), c))
            .collect()
    }
}

/// Compute the set of tiles that progress shanten by 1 if drawn.
pub fn waits(info: &PlayerInfo34) -> Waits {
    let cur = shanten::shanten(info);
    waits_with_target(info, cur - 1)
}

/// Compute waits whose draw drops shanten to exactly `target` (or below).
/// Used to accept "agari draws" (target = -2 against tenpai) consistently.
pub fn waits_with_target(info: &PlayerInfo34, target: Shanten) -> Waits {
    let left = info.compute_left_tiles();
    waits_for_counts(&info.hand, info.tehai_len_div3(), &left, target)
}

/// Lower-level: enumerate tile types whose addition produces shanten ≤ target.
///
/// `len_div3` is for the hand *before* adding the tile.
pub fn waits_for_counts(
    hand: &Counts34,
    len_div3: u8,
    left_tiles: &Counts34,
    target: Shanten,
) -> Waits {
    let hand_size: u8 = hand.iter().sum();
    let added_len_div3 = (hand_size + 1) / 3;
    let _ = len_div3;
    let mut out = Waits::new();
    let mut probe = *hand;
    for t in 0..TILE_COUNT {
        if probe[t] >= 4 {
            continue;
        }
        if left_tiles[t] == 0 {
            // Even if the algebra accepts this tile, no copies remain — skip.
            continue;
        }
        probe[t] += 1;
        let s = shanten::shanten_from_counts(&probe, added_len_div3);
        probe[t] -= 1;
        if s <= target {
            out.insert(t as u8, left_tiles[t]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::hand::PlayerInfo34Builder;
    use crate::analysis::tile::Tile34;
    use riichienv_core::hand_evaluator::HandEvaluator;

    fn from_mjai_list(list: &[&str]) -> PlayerInfo34 {
        PlayerInfo34Builder::new().add_many(list).build()
    }

    fn waits_set(w: &Waits) -> Vec<u8> {
        w.map.keys().copied().collect()
    }

    #[test]
    fn tenpai_pair_wait() {
        // 1-9m + 123p + 1s, waiting on 1s pair.
        let info = from_mjai_list(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "1s",
        ]);
        let w = waits(&info);
        let tiles = waits_set(&w);
        assert_eq!(tiles, vec![Tile34::from_mjai("1s").unwrap().idx()]);
        // 4 - 1 already held = 3 left
        assert_eq!(w.map[&Tile34::from_mjai("1s").unwrap().idx()], 3);
    }

    #[test]
    fn ryanmen_wait() {
        // 234m 567p 234s + 11z + 56s → waits 4s/7s (ryanmen).
        let info = from_mjai_list(&[
            "2m", "3m", "4m", "5p", "6p", "7p", "2s", "3s", "4s", "E", "E", "5s", "6s",
        ]);
        let w = waits(&info);
        let tiles = waits_set(&w);
        let four_s = Tile34::from_mjai("4s").unwrap().idx();
        let seven_s = Tile34::from_mjai("7s").unwrap().idx();
        assert!(tiles.contains(&four_s));
        assert!(tiles.contains(&seven_s));
    }

    #[test]
    fn waits_match_riichienv_for_tenpai() {
        // Cross-check our enumeration against riichienv's own wait function.
        let info = from_mjai_list(&[
            "2m", "3m", "4m", "5p", "6p", "7p", "2s", "3s", "4s", "E", "E", "5s", "6s",
        ]);
        let mine: Vec<u8> = waits_set(&waits(&info));

        // Build the same hand as a 136-tile vec for HandEvaluator.
        let mut tile_136s: Vec<u8> = Vec::new();
        for (idx, c) in info.hand.iter().enumerate() {
            for k in 0..*c {
                tile_136s.push((idx as u8) * 4 + k);
            }
        }
        let eval = HandEvaluator::new(tile_136s, vec![]);
        let mut theirs = eval.get_waits_u8();
        theirs.sort();
        assert_eq!(mine, theirs, "waits disagree with riichienv");
    }

    #[test]
    fn one_shanten_has_effective_tiles() {
        // 3 runs + 12p tatsu + 2 isolated honors = 1-shanten.
        let info = from_mjai_list(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "N", "P",
        ]);
        let cur = shanten::shanten(&info);
        assert_eq!(cur, 1);
        let w = waits(&info);
        assert!(!w.is_empty(), "expected effective tiles for 1-shanten");
        // Drawing 3p completes the run → progresses to tenpai.
        let three_p = Tile34::from_mjai("3p").unwrap().idx();
        assert!(w.map.contains_key(&three_p), "missing 3p in waits");
        // Pairing N or P also progresses (gives a head, drop a single).
        let n = Tile34::from_mjai("N").unwrap().idx();
        let p = Tile34::from_mjai("P").unwrap().idx();
        assert!(w.map.contains_key(&n) || w.map.contains_key(&p));
    }
}
