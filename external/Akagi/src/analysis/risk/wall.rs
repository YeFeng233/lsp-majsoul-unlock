//! Wall analysis: NC (no chance), OC (one chance), DNC (double no chance).
//!
//! Source: the mahjong-helper Go `risk_wall.go`. Each function takes
//! the 34-count `left_tiles` (live wall + opponents' hands) and returns the
//! list of tiles that are "safe" against ryanmen-shape waits because their
//! enabling neighbours are partially or fully unavailable.

use crate::analysis::hand::Counts34;
use crate::analysis::tile::TILE_COUNT;

/// Safety class assigned by the wall analysis. Order matches the Go source —
/// smaller is safer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WallSafeType {
    /// Only loses to single-wait pair / shanpon. (Strongest.)
    DoubleNoChance = 0,
    /// Doesn't lose to ryanmen — still vulnerable to kanchan/penchan/tanki.
    NoChance = 1,
    /// Both sides have all-thin walls (1 tile remaining each). Reasonably safe early.
    DoubleOneChance = 2,
    /// Mixed: one side double-thin, the other single-thin.
    MixedOneChance = 3,
    /// At least one side has 1 tile left.
    OneChance = 4,
}

/// Per-tile wall safety classification.
#[derive(Debug, Clone, Copy)]
pub struct WallSafeTile {
    pub tile34: u8,
    pub kind: WallSafeType,
}

fn nc(left: &Counts34, idx: usize) -> bool {
    left[idx] == 0
}

fn nc_or(left: &Counts34, idxs: &[usize]) -> bool {
    idxs.iter().any(|&i| nc(left, i))
}

fn nc_and(left: &Counts34, idxs: &[usize]) -> bool {
    idxs.iter().all(|&i| nc(left, i))
}

fn oc(left: &Counts34, idx: usize) -> bool {
    left[idx] == 1
}

fn oc_or(left: &Counts34, idxs: &[usize]) -> bool {
    idxs.iter().any(|&i| oc(left, i))
}

fn oc_and(left: &Counts34, idxs: &[usize]) -> bool {
    idxs.iter().all(|&i| oc(left, i))
}

/// Tiles that don't lose to ryanmen waits because both supporting tiles
/// are extinct (count 0).
pub fn no_chance(left: &Counts34) -> Vec<WallSafeTile> {
    let mut out = Vec::new();
    for suit in 0..3usize {
        for j in 0..3usize {
            let idx = 9 * suit + j;
            if nc_or(left, &[idx + 1, idx + 2]) {
                out.push(WallSafeTile {
                    tile34: idx as u8,
                    kind: WallSafeType::NoChance,
                });
            }
        }
        for j in 3..6usize {
            let idx = 9 * suit + j;
            if nc_or(left, &[idx - 2, idx - 1]) && nc_or(left, &[idx + 1, idx + 2]) {
                out.push(WallSafeTile {
                    tile34: idx as u8,
                    kind: WallSafeType::NoChance,
                });
            }
        }
        for j in 6..9usize {
            let idx = 9 * suit + j;
            if nc_or(left, &[idx - 2, idx - 1]) {
                out.push(WallSafeTile {
                    tile34: idx as u8,
                    kind: WallSafeType::NoChance,
                });
            }
        }
    }
    out
}

/// Tiles that *probably* don't lose to ryanmen because the supporting tiles
/// are reduced to 1 each (early-game heuristic).
pub fn one_chance(left: &Counts34) -> Vec<WallSafeTile> {
    let mut out = Vec::new();
    for suit in 0..3usize {
        for j in 0..3usize {
            let idx = 9 * suit + j;
            if oc_and(left, &[idx + 1, idx + 2]) {
                out.push(WallSafeTile {
                    tile34: idx as u8,
                    kind: WallSafeType::DoubleOneChance,
                });
            } else if oc_or(left, &[idx + 1, idx + 2]) {
                out.push(WallSafeTile {
                    tile34: idx as u8,
                    kind: WallSafeType::OneChance,
                });
            }
        }
        for j in 3..6usize {
            let idx = 9 * suit + j;
            let left_side = oc_or(left, &[idx - 2, idx - 1]);
            let right_side = oc_or(left, &[idx + 1, idx + 2]);
            if left_side && right_side {
                let kind = if oc_and(left, &[idx - 2, idx - 1, idx + 1, idx + 2]) {
                    WallSafeType::DoubleOneChance
                } else if oc_and(left, &[idx - 2, idx - 1]) || oc_and(left, &[idx + 1, idx + 2]) {
                    WallSafeType::MixedOneChance
                } else {
                    WallSafeType::OneChance
                };
                out.push(WallSafeTile {
                    tile34: idx as u8,
                    kind,
                });
            }
        }
        for j in 6..9usize {
            let idx = 9 * suit + j;
            if oc_and(left, &[idx - 2, idx - 1]) {
                out.push(WallSafeTile {
                    tile34: idx as u8,
                    kind: WallSafeType::DoubleOneChance,
                });
            } else if oc_or(left, &[idx - 2, idx - 1]) {
                out.push(WallSafeTile {
                    tile34: idx as u8,
                    kind: WallSafeType::OneChance,
                });
            }
        }
    }
    out
}

/// Tiles that only lose to single-wait pair / shanpon — wall extinguishes
/// every shape that would use them as a side tile of a sequence.
pub fn double_no_chance(left: &Counts34) -> Vec<WallSafeTile> {
    let mut out = Vec::new();
    let kind = WallSafeType::DoubleNoChance;
    for suit in 0..3usize {
        // 1: 2 or 3 extinct → 1 is DNC
        if nc_or(left, &[9 * suit + 1, 9 * suit + 2]) {
            out.push(WallSafeTile {
                tile34: (9 * suit) as u8,
                kind,
            });
        }
        // 2: 3 extinct, or both 1 and 4 extinct → 2 is DNC
        if nc(left, 9 * suit + 2) || nc_and(left, &[9 * suit, 9 * suit + 3]) {
            out.push(WallSafeTile {
                tile34: (9 * suit + 1) as u8,
                kind,
            });
        }
        // 3..=7: both supporting pairs extinct on either flank
        for j in 2..=6usize {
            let idx = 9 * suit + j;
            let cond_a = idx >= 2 && nc_and(left, &[idx - 2, idx + 1]);
            let cond_b = idx >= 1 && nc_and(left, &[idx - 1, idx + 1]);
            let cond_c = idx >= 1 && nc_and(left, &[idx - 1, idx + 2]);
            if cond_a || cond_b || cond_c {
                out.push(WallSafeTile {
                    tile34: idx as u8,
                    kind,
                });
            }
        }
        // 8: 7 extinct, or both 6 and 9 extinct
        if nc(left, 9 * suit + 6) || nc_and(left, &[9 * suit + 5, 9 * suit + 8]) {
            out.push(WallSafeTile {
                tile34: (9 * suit + 7) as u8,
                kind,
            });
        }
        // 9: 7 or 8 extinct
        if nc_or(left, &[9 * suit + 6, 9 * suit + 7]) {
            out.push(WallSafeTile {
                tile34: (9 * suit + 8) as u8,
                kind,
            });
        }
    }
    out
}

/// DNC analysis augmented with discards on the river: if `safe[idx+3]` is set
/// and one neighbour is a wall, the tile drops to DNC even when raw walls
/// alone wouldn't qualify.
pub fn double_no_chance_with_discards(
    left: &Counts34,
    safe_tiles_34: &[bool; TILE_COUNT],
) -> Vec<WallSafeTile> {
    let mut out = double_no_chance(left);
    let kind = WallSafeType::DoubleNoChance;
    for suit in 0..3usize {
        for j in 1..3usize {
            let idx = 9 * suit + j;
            if nc(left, idx - 1) && safe_tiles_34[idx + 3] {
                out.push(WallSafeTile {
                    tile34: idx as u8,
                    kind,
                });
            }
        }
        for j in 3..6usize {
            let idx = 9 * suit + j;
            if nc(left, idx - 1) && safe_tiles_34[idx + 3]
                || nc(left, idx + 1) && safe_tiles_34[idx - 3]
            {
                out.push(WallSafeTile {
                    tile34: idx as u8,
                    kind,
                });
            }
        }
        for j in 6..8usize {
            let idx = 9 * suit + j;
            if nc(left, idx + 1) && safe_tiles_34[idx - 3] {
                out.push(WallSafeTile {
                    tile34: idx as u8,
                    kind,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full() -> Counts34 {
        [4u8; TILE_COUNT]
    }

    #[test]
    fn no_chance_with_2m_wall() {
        let mut left = full();
        left[1] = 0; // 2m extinct
        let nc = no_chance(&left);
        // 1m must be NC because 2m is gone (it's also adjacent to 3m).
        assert!(nc.iter().any(|t| t.tile34 == 0));
    }

    #[test]
    fn one_chance_collects_thin_walls() {
        let mut left = full();
        left[1] = 1; // 2m thin
        left[2] = 1; // 3m thin
        let oc = one_chance(&left);
        // 1m should appear with DoubleOneChance because 2m AND 3m are thin.
        let entry = oc.iter().find(|t| t.tile34 == 0).expect("1m oc");
        assert_eq!(entry.kind, WallSafeType::DoubleOneChance);
    }

    #[test]
    fn dnc_with_discards_uses_safe_flag() {
        let mut left = full();
        left[1] = 0; // 2m extinct
        let mut safe = [false; TILE_COUNT];
        safe[7] = true; // 8m on our river → 8m is genbutsu

        let baseline = double_no_chance(&left);
        let augmented = double_no_chance_with_discards(&left, &safe);
        // 5m: nc(idx-1 = 4m)? false. nc(idx+1 = 6m) && safe[idx-3 = 2m]? safe[2]=false. So no add.
        // The augmentation hits 1m: nc(idx-1) — only j>=1, doesn't apply.
        // The genbutsu+wall rule fires for 5m when nc(6m) AND safe[2m] are both
        // true; with our setup safe[2m]=false, so no augmentation here.
        // Asserting equal length verifies the augmentation correctly inspects
        // both wall + safe before adding entries.
        assert_eq!(baseline.len(), augmented.len());
    }
}
