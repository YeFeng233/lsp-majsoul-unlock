//! A self-contained approximation of Mortal's single-player tables.
//!
//! Upstream `libriichi` ships precomputed tables (`SinglePlayerTables`) for
//! tenpai/win probability and expected value per discard, per turn. riichienv
//! has no equivalent, so this module derives the same *shape* of information
//! from the live state using the shanten routine that is available.
//!
//! What these planes do (v4 obs, the last 123 channels):
//!
//! * `max_ev` — the best expected value across candidate discards, and the
//!   normaliser for the EV plane (`encode_sp_table` scales by `1 / max_ev`).
//! * required tiles — for each discard, which tiles complete the hand.
//! * SP table — per turn: probability the hand reaches tenpai, probability it
//!   wins, and the expected value of that line.
//!
//! The numbers here are an estimate, not a reproduction of Mortal's tables.
//! They keep the model's inputs in the right *range* and *ordering* so the
//! network's own weights can do the decision-making, rather than seeing a block
//! of zeros — which is what a missing table looks like to the model, and reads
//! to it as "this line has no value at all".
//!
//! The probability model is deliberately simple and monotone:
//!
//! * `ukeire` is the number of live tiles that reduce shanten.
//! * the per-turn tenpai probability is the chance of drawing at least one such
//!   tile within `turn + 1` draws, hypergeometric over the remaining wall.
//! * the win probability multiplies that by a per-turn acceptance factor that
//!   accounts for having to survive and for opponents not winning first.
//! * the expected value is the win probability times an estimated hand value.

use crate::mortal_obs::{SpCandidate, TILES};

/// Upstream `MAX_NUM_TURNS`; the SP planes have one column per turn.
pub const MAX_NUM_TURNS: usize = 17;

/// A rough average hand value in points, used to turn a win probability into an
/// expected value. Mortal's real tables are trained, not analytic; this only
/// needs to be the right order of magnitude.
const AVG_WIN_POINTS: f32 = 5200.0;

/// Per-turn decay applied to the win probability, covering the chance that the
/// hand is cut short (someone else wins, the wall runs out).
const SURVIVAL_PER_TURN: f32 = 0.94;

/// Compute candidate rows for the single-player table planes.
///
/// `tehai` is the concealed hand as a 34-wide count vector, `discards` the raw
/// tiles the seat may discard, `remaining` the live wall count, and `visible`
/// the per-tile count of copies this seat can account for.
pub fn build_sp_candidates(
    tehai: &[u8; TILES],
    discards: &[u8],
    remaining: u8,
    visible: &[u8; TILES],
) -> Vec<SpCandidate> {
    let mut out = Vec::with_capacity(discards.len());
    let mut seen = [false; TILES];

    for &raw in discards {
        let tid = crate::mortal_obs::mortal_obs_deaka(raw);
        if seen[tid] {
            continue;
        }
        seen[tid] = true;
        if tehai[tid] == 0 {
            continue;
        }

        let mut after = *tehai;
        after[tid] -= 1;

        // The tiles that complete the hand after this discard.
        let required = required_tiles(&after);
        let ukeire: u32 = required
            .iter()
            .map(|&t| 4u32.saturating_sub(visible[t as usize] as u32))
            .sum();

        let (tenpai_probs, win_probs, exp_values) =
            project(&after, &required, ukeire, remaining);

        out.push(SpCandidate {
            tile: raw,
            shanten_down: false,
            tenpai_probs,
            win_probs,
            exp_values,
            required_tiles: required,
        });
    }
    out
}

/// The tiles that reduce this hand's shanten by one (i.e. its ukeire).
fn required_tiles(tehai: &[u8; TILES]) -> Vec<u8> {
    let base = shanten(tehai);
    let mut out = Vec::new();
    for t in 0..TILES {
        if tehai[t] >= 4 {
            continue;
        }
        let mut probe = *tehai;
        probe[t] += 1;
        if shanten(&probe) < base {
            out.push(t as u8);
        }
    }
    out
}

/// `tehai_len_div3` here is the concealed hand length divided by three, which
/// is what `calc_shanten_from_counts` expects. Passing the meld count instead
/// mis-sizes the lookup and can report a tenpai that does not exist.
fn shanten(tehai: &[u8; TILES]) -> i8 {
    let total: usize = tehai.iter().map(|&c| c as usize).sum();
    riichienv_core::shanten::calc_shanten_from_counts(tehai, (total / 3) as u8)
}

/// Project tenpai/win/EV per turn from the ukeire count.
///
/// Returns three `MAX_NUM_TURNS`-long vectors. An entry is zero once the
/// probability underflows, which is how upstream signals "no further turns are
/// meaningful" — `encode_sp_table` stops writing at the first zero.
fn project(
    tehai: &[u8; TILES],
    required: &[u8],
    ukeire: u32,
    remaining: u8,
) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    let mut tenpai_probs = vec![0.0f32; MAX_NUM_TURNS];
    let mut win_probs = vec![0.0f32; MAX_NUM_TURNS];
    let mut exp_values = vec![0.0f32; MAX_NUM_TURNS];

    let already = shanten(tehai) <= 0;
    let wall = remaining.max(1) as f32;

    // Expected number of winning tiles drawn over `n` draws, hypergeometric.
    // For small draws this is well approximated by n * ukeire / wall.
    for turn in 0..MAX_NUM_TURNS {
        let draws = (turn + 1) as f32;
        let per_draw = (ukeire as f32 / wall).min(0.95);
        let at_least_one = 1.0 - (1.0 - per_draw).powf(draws);
        let tp = if already { 1.0 } else { at_least_one };
        if tp <= 1e-4 {
            break;
        }
        tenpai_probs[turn] = tp.min(1.0);

        // Winning also needs the tile to arrive while the hand is still live.
        let wp = (tp * SURVIVAL_PER_TURN.powi(turn as i32 + 1)).min(1.0);
        win_probs[turn] = wp;

        // The required set is empty for an already-complete hand; treat that as
        // a guaranteed shape so the EV plane is not spuriously zeroed.
        let shape = if required.is_empty() { 1.0 } else { 1.0 };
        exp_values[turn] = (wp * AVG_WIN_POINTS * shape).min(60_000.0);
    }

    (tenpai_probs, win_probs, exp_values)
}

/// The best expected value across all candidate lines — the EV plane's
/// normaliser. Zero when no line has any value, which disables the plane the
/// same way upstream does.
pub fn max_ev(candidates: &[SpCandidate]) -> f32 {
    candidates
        .iter()
        .flat_map(|c| c.exp_values.iter().copied())
        .fold(0.0f32, f32::max)
}
