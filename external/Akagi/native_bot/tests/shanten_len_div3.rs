//! Regression guard for the `tehai_len_div3` argument to
//! `calc_shanten_from_counts`.
//!
//! That parameter is the *concealed hand length divided by three*, not the meld
//! count. `calc_normal` uses it as a table index and `calc_shanten_from_counts`
//! gates the chiitoitsu/kokushi branches on `>= 4`, so passing the meld count
//! makes a closed hand look like an open one. The error is not subtle: a
//! scattered 13-tile hand reports tenpai.
//!
//! Mortal feeds shanten straight into an observation channel, and a wrong value
//! there changes which discard looks attractive without any error surfacing.

use riichienv_core::shanten::calc_shanten_from_counts;

const T: usize = 34;

fn counts(spec: &[(usize, u8)]) -> [u8; T] {
    let mut c = [0u8; T];
    for &(t, n) in spec {
        c[t] = n;
    }
    c
}

fn correct(hand: &[u8; T]) -> i8 {
    let total: usize = hand.iter().map(|&x| x as usize).sum();
    calc_shanten_from_counts(hand, (total / 3) as u8)
}

#[test]
fn scattered_hand_is_not_reported_as_tenpai() {
    // 13 tiles, no structure at all: 111m 5m 9m 1p 5p 9p 2s 5s 8s E S W N
    // (kept at 13 by dropping the triplet's excess)
    let hand = counts(&[
        (0, 1), (4, 1), (8, 1), (9, 1), (13, 1), (17, 1),
        (19, 1), (22, 1), (25, 1), (27, 1), (28, 1), (29, 1), (30, 1),
    ]);
    let total: usize = hand.iter().map(|&x| x as usize).sum();
    assert_eq!(total, 13);

    let good = correct(&hand);
    let bad = calc_shanten_from_counts(&hand, 0);
    assert!(
        good >= 3,
        "a scattered 13-tile hand is many tiles from tenpai, got {good}"
    );
    assert_ne!(
        good, bad,
        "the meld-count form must differ here, otherwise the guard is vacuous"
    );
}

#[test]
fn chiitoitsu_is_reachable_for_a_closed_hand() {
    // Seven pairs, 14 tile-slots: 1122m 3344p 5566s 77s is 13 tiles + 1 only
    // when counting the pair twice, so use the standard 13-tile form.
    let hand = counts(&[(0, 2), (1, 2), (9, 2), (10, 2), (18, 2), (19, 2), (20, 2)]);
    let total: usize = hand.iter().map(|&x| x as usize).sum();
    assert_eq!(total, 14);
    let good = calc_shanten_from_counts(&hand, (total / 3) as u8);
    // Seven pairs is complete here (14 tiles, 7 pairs) -> -1.
    assert_eq!(good, -1, "seven pairs on 14 tiles is a winning hand");
}

#[test]
fn open_hand_uses_its_own_length() {
    // 8 concealed tiles: 11m 55m 1p 5p (four pairs, 8 tiles) -> len/3 = 2.
    let hand = counts(&[(0, 2), (4, 2), (9, 2), (13, 2)]);
    let total: usize = hand.iter().map(|&x| x as usize).sum();
    assert_eq!(total, 8);
    let good = correct(&hand);
    assert!(good >= 1, "four pairs is at least 1 shanten, got {good}");
    // The meld-count form is wrong for this shape.
    assert_ne!(good, calc_shanten_from_counts(&hand, 1));
}
