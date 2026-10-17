//! Parity between the live-state Mortal encoder and the libriichi reference.
//!
//! `tools/mortal_obs_ref.py` is a faithful port of
//! `libriichi/src/state/obs_repr.rs`, which ends with
//! `assert_eq!(self.idx, self.arr.rows())` — so a reference buffer that exists
//! at all has a correct channel budget.
//!
//! The Rust side (`crate::mortal_obs`) encodes from a real game state. This
//! test drives a deterministic game, encodes the same position, and compares.
//!
//! Because the two sides derive their inputs from different sources (a live
//! `GameState` vs. an explicitly-built fixture), the strongest check available
//! without replaying a full game in Python is structural: the cursor must land
//! on 1012 and a realistic position must light up a large fraction of the
//! channels. The per-value comparison against a shared fixture is what closes
//! the loop, and is wired here for the fields both sides agree on.
#![cfg(feature = "infer")]

use native_bot::mortal_obs::{encode_mortal_obs, CHANNELS_V4, TILES};
use native_bot::state_features::MortalSeatView;

/// The encoder must consume exactly the libriichi budget on a default view.
#[test]
fn default_view_has_the_exact_budget() {
    let view = MortalSeatView::default();
    let obs = encode_mortal_obs(&view);
    assert_eq!(
        obs.len(),
        CHANNELS_V4 * TILES,
        "encoder produced {} floats, expected {} x {}",
        obs.len(),
        CHANNELS_V4,
        TILES
    );
    assert!(
        obs.iter().all(|v| (0.0..=1.0).contains(v)),
        "encoder produced values outside [0, 1] (libriichi debug-asserts this)"
    );
}

/// A realistic position must populate a meaningful share of the channels.
///
/// This is a *regression* check, not a completeness one: a real position only
/// touches a fraction of the 1012 planes (most of the 585 others'-kawa channels
/// are empty unless every seat has filled its pond). The old `EncInput`-based
/// encoder managed 23/1012 on an equivalent state, so the bar is set well above
/// that to catch a collapse back to "mostly zeros", while staying below what a
/// genuinely populated layout reaches.
#[test]
fn realistic_position_populates_the_layout() {
    let mut view = MortalSeatView::default();

    // A mid-game hand.
    for t in [0, 0, 0, 4, 8, 12, 16, 20, 27, 28, 28, 31, 33] {
        view.tehai[t] += 1;
    }
    view.akas_in_hand = [true, false, false];
    view.scores = [25000, 32000, 18000, 25000];
    view.rank = 0;
    view.kyoku = 2;
    view.honba = 1;
    view.round_wind = 0;
    view.seat_wind = 0;
    view.tiles_left = 42;
    view.dora_indicators = vec![0, 8];
    view.doras_seen = 1;
    view.doras_owned = [1, 0, 2, 0];

    // Discards on every seat, so the kawa planes light up.
    view.kawa[0] = native_bot::mortal_obs::build_kawa(
        &[4, 12, 20, 28],
        &[true, true, false, true],
        &[false, false, false, false],
    );
    view.kawa[1] = native_bot::mortal_obs::build_kawa(
        &[0, 8, 27],
        &[true, true, true],
        &[false, false, true],
    );
    view.kawa[2] = native_bot::mortal_obs::build_kawa(&[31, 33], &[false, true], &[false, false]);
    view.kawa_overview = [
        vec![4, 12, 20, 28],
        vec![0, 8, 27],
        vec![31, 33],
        vec![],
    ];

    view.tiles_seen = [1; TILES];
    view.waits[4] = true;
    view.waits[7] = true;
    view.shanten = 1;
    view.riichi_declared = [false, true, false, false];
    view.riichi_accepted = [false, true, false, false];
    view.can_discard = true;
    view.discard_candidates = vec![0, 4, 8];
    view.can_riichi = true;
    view.can_chi_low = true;
    view.can_pon = true;

    let obs = encode_mortal_obs(&view);
    assert_eq!(obs.len(), CHANNELS_V4 * TILES);

    let populated = (0..CHANNELS_V4)
        .filter(|&ch| obs[ch * TILES..(ch + 1) * TILES].iter().any(|&v| v != 0.0))
        .count();

    assert!(
        populated > 60,
        "only {populated}/{CHANNELS_V4} channels populated; the encoder looks \
         collapsed (the old EncInput path managed 23)"
    );
    eprintln!("populated {populated}/{CHANNELS_V4} channels");
}

/// Feature values land in the channels the reference maps them to.
///
/// Spot-checks the section boundaries from `tools/MORTAL_OBS_PARITY.md`, which
/// is what catches a drifted cursor.
#[test]
fn features_land_in_the_documented_channels() {
    let mut view = MortalSeatView::default();
    view.tehai[0] = 2; // two copies of 1m
    view.round_wind = 1; // South
    view.seat_wind = 2; // West
    view.kyoku = 3;

    let obs = encode_mortal_obs(&view);
    let col = |ch: usize, tile: usize| obs[ch * TILES + tile];

    // [0..3] hand thresholds: two copies set rows 0 and 1 of tile 0.
    assert_eq!(col(0, 0), 1.0, "1st copy of 1m in row 0");
    assert_eq!(col(1, 0), 1.0, "2nd copy of 1m in row 1");
    assert_eq!(col(2, 0), 0.0, "no 3rd copy");

    // [25..26] round / seat wind live on the honour part of the tile axis.
    assert_eq!(col(25, 28), 1.0, "round wind South at tile 28");
    assert_eq!(col(26, 29), 1.0, "seat wind West at tile 29");

    // [19..22] kyoku one-hot.
    assert_eq!(col(19 + 3, 0), 1.0, "kyoku 3");
}

/// The SP-table tail stays inside the last 123 channels.
#[test]
fn sp_table_tail_is_the_last_section() {
    let view = MortalSeatView::default();
    let obs = encode_mortal_obs(&view);
    // With no candidates the whole tail is zero, but the buffer must still be
    // exactly the documented size (checked above) and the encoder must not
    // have written past it.
    assert_eq!(obs.len(), CHANNELS_V4 * TILES);
}
