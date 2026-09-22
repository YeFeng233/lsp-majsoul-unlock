//! 14-tile discard search.
//!
//! Given a 14-tile state, enumerate every possible discard, compute the
//! resulting 13-tile analysis, and return ranked candidate lists.
//! Mirrors `CalculateShantenWithImproves14` in the Go reference.

use super::hand::PlayerInfo34;
use super::improves::analyze_13;
use super::result::{DiscardCandidate, Hand14Result};
use super::shanten::{shanten, shanten_from_counts};
use super::tile::{Tile34, TILE_COUNT};

/// Build the candidate lists from a 14-tile state.
pub fn analyze_14(info: &PlayerInfo34) -> Hand14Result {
    let cur_shanten = shanten(info);
    let len_div3 = info.tehai_len_div3();

    let mut maintain: Vec<DiscardCandidate> = Vec::new();
    let mut backwards: Vec<DiscardCandidate> = Vec::new();

    let mut probe = info.clone();
    for d in 0..TILE_COUNT {
        if probe.hand[d] == 0 {
            continue;
        }
        probe.hand[d] -= 1;
        // Resulting 13-tile shanten:
        let post_shanten = shanten_from_counts(&probe.hand, len_div3);
        // Skip impossibly bad discards (more than +1 shanten).
        let bucket = if post_shanten == cur_shanten {
            Some(&mut maintain)
        } else if post_shanten == cur_shanten + 1 {
            Some(&mut backwards)
        } else {
            None
        };

        if let Some(b) = bucket {
            // analyze_13 expects a closed-hand state; cloning probe is enough
            // because melds / dora / opponents are unchanged by discarding
            // from our hand.
            let result = analyze_13(&probe);
            b.push(DiscardCandidate {
                discard: Tile34(d as u8).to_mjai().to_string(),
                result,
            });
        }
        probe.hand[d] += 1;
    }

    // Sort: prefer larger mixed_waits_score, then larger waits_total, then
    // larger avg_agari_rate as tiebreakers.
    let cmp = |a: &DiscardCandidate, b: &DiscardCandidate| {
        b.result
            .mixed_waits_score
            .partial_cmp(&a.result.mixed_waits_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.result.waits_total.cmp(&a.result.waits_total))
            .then_with(|| {
                b.result
                    .avg_agari_rate
                    .partial_cmp(&a.result.avg_agari_rate)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    };
    maintain.sort_by(cmp);
    backwards.sort_by(cmp);

    Hand14Result {
        shanten: cur_shanten,
        maintain,
        backwards,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::hand::PlayerInfo34Builder;

    #[test]
    fn fourteen_tiles_produces_candidates() {
        // 234m 234p 234s 678p + EE pair + extra E (14 tiles)
        let info = PlayerInfo34Builder::new()
            .add_many(&[
                "2m", "3m", "4m", "2p", "3p", "4p", "6p", "7p", "8p", "2s", "3s", "4s", "E", "E",
            ])
            .build();
        assert_eq!(info.hand_size(), 14);
        let r = analyze_14(&info);
        assert_eq!(r.shanten, -1, "winning shape should be agari");
    }

    #[test]
    fn discard_candidate_for_one_shanten_14() {
        // 1-shanten 14: 1-9m + 1-2p + N + P + extra (we need 14 tiles)
        // Take the 1-shanten 13 from earlier and add an extra tile to make 14.
        let info = PlayerInfo34Builder::new()
            .add_many(&[
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "N", "P", "P",
            ])
            .build();
        assert_eq!(info.hand_size(), 14);
        let r = analyze_14(&info);
        // current shanten = 1 (since the previous 13-state was 1-shanten, adding
        // a 2nd P actually gives a pair → tenpai).
        // Discarding N and keeping the new pair should be the top-rated maintain.
        assert!(r.shanten <= 1);
        assert!(!r.maintain.is_empty());
        // Top candidate should have the highest mixed_waits_score.
        let top = &r.maintain[0];
        for cand in r.maintain.iter().skip(1) {
            assert!(top.result.mixed_waits_score >= cand.result.mixed_waits_score - 1e-9);
        }
    }
}
