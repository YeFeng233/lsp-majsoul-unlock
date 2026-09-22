//! Shanten calculation.
//!
//! Wraps `riichienv_core::shanten::calc_shanten_from_counts` so the rest of
//! the analysis engine never touches riichienv's API directly. Open hands
//! work because riichienv's shanten table accepts arbitrary `tehai_len_div3`
//! and we feed only the *closed* tiles in: each open meld is implicitly
//! one completed set, so the closed-portion shanten is exactly what we want.
//!
//! Special case: chitoitsu / kokushi are only legal for fully-closed 13/14
//! tile hands. riichienv enforces this via `tehai_len_div3 < 4` skipping
//! both checks; we mirror that and never report chitoi/kokushi when the
//! caller has open melds.

use riichienv_core::shanten as rshanten;

use super::hand::{Counts34, PlayerInfo34};

/// Shanten value: -1 means agari, 0 means tenpai, 1..=8 means N tiles away.
pub type Shanten = i8;

/// Compute shanten for a player.
pub fn shanten(info: &PlayerInfo34) -> Shanten {
    shanten_from_counts(&info.hand, info.tehai_len_div3())
}

/// Lower-level entry: shanten for a raw 34-counts vector.
///
/// `len_div3` should be `total_tile_count / 3` (floor) — same convention as
/// riichienv. For 13 closed → 4. For 10 closed (1 open meld) → 3.
pub fn shanten_from_counts(counts: &Counts34, len_div3: u8) -> Shanten {
    rshanten::calc_shanten_from_counts(counts, len_div3)
}

/// Shanten for a hypothetical hand: take the player's current closed hand,
/// add `tile`, and recompute. Returns the new shanten value.
///
/// Useful when probing per-tile waits at any shanten level.
pub fn shanten_with_added(info: &PlayerInfo34, tile: u8) -> Option<Shanten> {
    if (tile as usize) >= super::tile::TILE_COUNT {
        return None;
    }
    if info.hand[tile as usize] >= 4 {
        return None;
    }
    let mut counts = info.hand;
    counts[tile as usize] += 1;
    let len = info.hand_size() + 1;
    Some(shanten_from_counts(&counts, len / 3))
}

/// Shanten for a hypothetical hand after a discard: remove `tile`, recompute.
pub fn shanten_with_removed(info: &PlayerInfo34, tile: u8) -> Option<Shanten> {
    if (tile as usize) >= super::tile::TILE_COUNT {
        return None;
    }
    if info.hand[tile as usize] == 0 {
        return None;
    }
    let mut counts = info.hand;
    counts[tile as usize] -= 1;
    let len = info.hand_size() - 1;
    Some(shanten_from_counts(&counts, len / 3))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::hand::{Meld34, Meld34Kind, PlayerInfo34Builder};
    use crate::analysis::tile::Tile34;

    fn from_mjai_list(list: &[&str]) -> PlayerInfo34 {
        PlayerInfo34Builder::new().add_many(list).build()
    }

    #[test]
    fn agari_hand_returns_minus_one() {
        let info = from_mjai_list(&[
            "1m", "1m", "1m", "2m", "3m", "4m", "5p", "6p", "7p", "8s", "9s", "7s", "E", "E",
        ]);
        // 14 tiles, valid winning shape (123m + 1m1m1m + 567p + 789s + EE)
        assert_eq!(shanten(&info), -1);
    }

    #[test]
    fn tenpai_returns_zero() {
        // 13 tiles, missing 1 tile to complete: waiting on 1m for 1m1m
        let info = from_mjai_list(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "1s",
        ]);
        // Pair on 1s missing — 0-shanten waiting on 1s.
        assert_eq!(shanten(&info), 0);
    }

    #[test]
    fn one_shanten() {
        // 3 runs + 12p tatsu + 2 isolated honors → 1-shanten.
        let info = from_mjai_list(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "N", "P",
        ]);
        assert_eq!(shanten(&info), 1);
    }

    #[test]
    fn chiitoitsu_one_shanten() {
        let info = from_mjai_list(&[
            "1m", "1m", "3m", "3m", "5p", "5p", "7p", "7p", "1s", "1s", "9s", "9s", "E",
        ]);
        // Six pairs + lone E → chiitoi 0-shanten waiting on E.
        // Confirm chitoi path is reached.
        assert_eq!(shanten(&info), 0);
    }

    #[test]
    fn kokushi_tenpai() {
        let info = from_mjai_list(&[
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C",
        ]);
        // 13-wait kokushi → tenpai.
        assert_eq!(shanten(&info), 0);
    }

    #[test]
    fn open_hand_with_pon_reduces_required_sets() {
        // Pon(1m) + closed [234p, 567p, 11s, 55s] = 13 tiles total → tenpai
        // (3 sets + 2 pairs; need to upgrade one pair to a set, keep the other as head).
        let info = PlayerInfo34Builder::new()
            .add_many(&["2p", "3p", "4p", "5p", "6p", "7p", "1s", "1s", "5s", "5s"])
            .meld(Meld34 {
                kind: Meld34Kind::Pon,
                tiles: vec![Tile34::from_mjai("1m").unwrap()],
                called_tile: Some(Tile34::from_mjai("1m").unwrap()),
                from_who: Some(2),
                aka_count: 0,
            })
            .build();
        assert_eq!(info.hand_size(), 10);
        assert_eq!(info.tehai_len_div3(), 3);
        // Closed-portion shanten with len_div3=3 should be 0 (tenpai on 1s/5s shanpon).
        assert_eq!(shanten(&info), 0);
    }

    #[test]
    fn open_hand_one_shanten() {
        // Same as above, but break one of the runs to drop to 1-shanten.
        let info = PlayerInfo34Builder::new()
            .add_many(&["2p", "3p", "4p", "5p", "6p", "8p", "1s", "1s", "5s", "5s"])
            .meld(Meld34 {
                kind: Meld34Kind::Pon,
                tiles: vec![Tile34::from_mjai("1m").unwrap()],
                called_tile: Some(Tile34::from_mjai("1m").unwrap()),
                from_who: Some(2),
                aka_count: 0,
            })
            .build();
        assert_eq!(shanten(&info), 1);
    }

    #[test]
    fn shanten_with_added_progresses() {
        let info = from_mjai_list(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "9s",
        ]);
        let before = shanten(&info);
        // Drawing 9s should complete pair → -1 shanten if discard 9s? No — added 14th tile,
        // so 14-tile shanten of pair completion is -1.
        let after = shanten_with_added(&info, Tile34::from_mjai("9s").unwrap().idx()).unwrap();
        assert!(after < before, "before={before} after={after}");
        assert_eq!(after, -1);
    }
}
