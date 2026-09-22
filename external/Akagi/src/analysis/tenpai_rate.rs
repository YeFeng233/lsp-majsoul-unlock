//! Open-hand tenpai-rate estimate.
//!
//! Source: the mahjong-helper Go `tenpai_rate.go`. Closed (dama) hands
//! are approximated by the turn count; open hands look up
//! `data::tenpai::rate(melds, turn, tedashi)`.

use super::data::tenpai;
use super::hand::{Meld34, Meld34Kind};

/// Estimate tenpai rate (0..100) for an opponent.
///
/// `melds` — opponent's open + concealed melds.
/// `discard_count` — how many tiles they've discarded so far.
/// `meld_discards_at` — the index in their discard list at which each meld
/// happened (so we can count tedashi after the last call).
/// `tedashi_count` — number of hand-cut (manual) discards after the last
/// call. The caller computes this from per-discard tedashi/tsumogiri flags.
pub fn estimate(
    melds: &[Meld34],
    discard_count: usize,
    meld_discards_at: &[usize],
    tedashi_count: usize,
) -> f64 {
    let opened: Vec<&Meld34> = melds
        .iter()
        .filter(|m| !matches!(m.kind, Meld34Kind::Ankan))
        .collect();
    let is_open = !opened.is_empty();
    if !is_open {
        // Damaten heuristic — closer to "turn count" early on.
        return discard_count as f64;
    }
    if opened.len() >= 4 {
        return 100.0;
    }

    // Decide the table row from the last tedashi-after-call window.
    let _ = meld_discards_at; // already pre-computed by caller.
    tenpai::rate(opened.len(), discard_count, tedashi_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::hand::Meld34Kind;
    use crate::analysis::tile::Tile34;

    fn pon(tile: &str) -> Meld34 {
        Meld34 {
            kind: Meld34Kind::Pon,
            tiles: vec![Tile34::from_mjai(tile).unwrap()],
            called_tile: Some(Tile34::from_mjai(tile).unwrap()),
            from_who: Some(2),
            aka_count: 0,
        }
    }

    #[test]
    fn dama_returns_turn_count() {
        let r = estimate(&[], 7, &[], 0);
        assert_eq!(r, 7.0);
    }

    #[test]
    fn one_meld_turn_5_tedashi_2() {
        let r = estimate(&[pon("E")], 5, &[1], 2);
        assert!((r - 19.88).abs() < 1e-6);
    }

    #[test]
    fn ankan_only_counts_as_dama() {
        // Ankan does not make the hand "naki" for tenpai-rate purposes —
        // riichi-after-ankan stays a dama heuristic.
        let ankan = Meld34 {
            kind: Meld34Kind::Ankan,
            tiles: vec![Tile34::from_mjai("E").unwrap()],
            called_tile: None,
            from_who: None,
            aka_count: 0,
        };
        let r = estimate(&[ankan], 7, &[], 0);
        assert_eq!(r, 7.0);
    }

    #[test]
    fn four_open_melds_means_definitely_tenpai() {
        let r = estimate(&[pon("E"), pon("S"), pon("W"), pon("N")], 5, &[], 0);
        assert_eq!(r, 100.0);
    }
}
