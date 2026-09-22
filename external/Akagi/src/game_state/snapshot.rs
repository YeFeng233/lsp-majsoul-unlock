//! UI-shaped snapshot of the live `riichienv_core::state::GameState`.
//!
//! `riichienv` exposes its internal state as raw `u8` tile IDs (0..136
//! with red-five sentinels at 16/52/88), `Vec<...>` everywhere, and a
//! `Meld` shape that mixes `meld_type` with positional arrays. None of
//! that is what a frontend wants. `GameStateSnapshot` flattens it into
//! mjai tile strings, fixed-size arrays where appropriate, and a small
//! enum for `phase`.
//!
//! All types are `Serialize + Deserialize` so they can ride the future
//! `GameStateBus` straight through to the frontend without further
//! transform.

use std::collections::HashMap;

use riichienv_core::parser::tid_to_mjai;
use riichienv_core::state::GameState;
use riichienv_core::state_3p::GameState3P;
use riichienv_core::types::{Meld, MeldType};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Active player's turn (post-draw, awaiting discard / kan / agari).
    WaitAct,
    /// Discard just landed; other seats may chi/pon/kan/ron.
    WaitResponse,
}

impl From<riichienv_core::action::Phase> for Phase {
    fn from(p: riichienv_core::action::Phase) -> Self {
        match p {
            riichienv_core::action::Phase::WaitAct => Phase::WaitAct,
            riichienv_core::action::Phase::WaitResponse => Phase::WaitResponse,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeldKind {
    Chi,
    Pon,
    Daiminkan,
    Ankan,
    Kakan,
}

impl From<MeldType> for MeldKind {
    fn from(m: MeldType) -> Self {
        match m {
            MeldType::Chi => MeldKind::Chi,
            MeldType::Pon => MeldKind::Pon,
            MeldType::Daiminkan => MeldKind::Daiminkan,
            MeldType::Ankan => MeldKind::Ankan,
            MeldType::Kakan => MeldKind::Kakan,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeldSnapshot {
    pub kind: MeldKind,
    /// mjai tile strings (e.g. `"5mr"`, `"P"`).
    pub tiles: Vec<String>,
    /// -1 for ankan/kakan (no claim source); 0..3 for opened melds.
    pub from_who: i8,
    pub called_tile: Option<String>,
}

impl From<&Meld> for MeldSnapshot {
    fn from(m: &Meld) -> Self {
        // riichienv stores meld tiles already mapped to 34-space inside
        // HandEvaluator, but the live PlayerState melds keep the raw 136
        // ids from the wall. tid_to_mjai handles both because we pass
        // the raw byte through unchanged.
        Self {
            kind: m.meld_type.into(),
            tiles: m.tiles.iter().copied().map(tid_to_mjai).collect(),
            from_who: m.from_who,
            called_tile: m.called_tile.map(tid_to_mjai),
        }
    }
}

/// One entry in a player's discard pile. Carries the full per-tile signal we
/// need for both rendering (mahgen `^`/`_`/`v` markers) and the analysis
/// engine's tedashi tracking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscardEntry {
    /// mjai tile string (e.g. `"5mr"`, `"P"`, `"3p"`).
    pub tile: String,
    /// `true` = manual cut (tedashi). `false` = drew-and-discarded (tsumogiri).
    pub tedashi: bool,
    /// `true` if this is the discard that committed riichi.
    pub is_riichi: bool,
    /// `true` if this discard was claimed by another player (pon/chi/kan) and
    /// has physically left the pond. **View-only**: the entry is kept in the
    /// list so the analysis engine still counts it as genbutsu/furiten against
    /// this seat; only the mahgen river encoder skips it. riichienv-core never
    /// removes called tiles from its `discards`, so we reconstruct this flag
    /// from the melds in [`GameStateSnapshot::from_state`].
    #[serde(default)]
    pub called: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerSnapshot {
    pub seat: u8,
    /// Concealed hand. mjai tile strings; closed seats other than the
    /// observer normally see `"?"`, but this snapshot reflects whatever
    /// the engine has tracked (it sees full info for the side it's
    /// fed).
    pub tehai: Vec<String>,
    pub melds: Vec<MeldSnapshot>,
    /// Discards in order. Each entry carries tedashi + riichi-commit flags.
    pub river: Vec<DiscardEntry>,
    pub score: i32,
    pub riichi_declared: bool,
    pub riichi_stage: bool,
    pub double_riichi: bool,
    /// Index in `river` of the riichi-committing discard, if any.
    pub riichi_declaration_index: Option<usize>,
    /// Kita (北抜き / nukidora) tiles set aside this round. 3p only;
    /// always empty in 4p. mjai tile strings (always `"N"` in practice).
    #[serde(default)]
    pub kita_tiles: Vec<String>,
    /// The just-drawn tile, if any. Populated for the active player only.
    #[serde(default)]
    pub drawn_tile: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameStateSnapshot {
    /// Round wind as mjai char: `"E"`, `"S"`, `"W"`, `"N"`.
    pub bakaze: String,
    /// 1..=4.
    pub kyoku: u8,
    pub honba: u8,
    pub kyotaku: u32,
    pub oya: u8,
    pub current_player: u8,
    pub turn_count: u32,
    pub phase: Phase,
    pub is_done: bool,
    /// 3 (sanma) or 4 (yonma).
    #[serde(default = "default_num_players")]
    pub num_players: u8,
    /// One entry per seat. Length matches `num_players`.
    pub players: Vec<PlayerSnapshot>,
    pub dora_markers: Vec<String>,
    /// The seat the active observer (our bot) plays as. Captured from the
    /// `start_game.id` field. `None` if the bridge didn't tag a perspective.
    pub our_seat: Option<u8>,
}

fn default_num_players() -> u8 {
    4
}

impl GameStateSnapshot {
    /// Build a snapshot from a 4-player engine state. `our_seat` is threaded
    /// in by the tracker — see [`crate::game_state::tracker`].
    pub fn from_state(s: &GameState, our_seat: Option<u8>) -> Self {
        let in_wait_act = matches!(s.phase, riichienv_core::action::Phase::WaitAct);
        let np = s.players.len();
        // Build every seat's melds up front so we can tell which discards were
        // claimed away (a meld's `from_who` is the seat the called tile came
        // from). riichienv-core keeps called tiles in `discards`, so the view
        // river hides them via the per-discard `called` flag.
        let all_melds: Vec<Vec<MeldSnapshot>> = (0..np)
            .map(|i| s.players[i].melds.iter().map(MeldSnapshot::from).collect())
            .collect();
        let mut called_by_seat = called_tiles_by_seat(&all_melds, np);
        let players: Vec<PlayerSnapshot> = all_melds
            .into_iter()
            .enumerate()
            .map(|(i, melds)| {
                let p = &s.players[i];
                let drawing = i as u8 == s.current_player && in_wait_act;
                let river = build_river(
                    &p.discards,
                    &p.discard_from_hand,
                    &p.discard_is_riichi,
                    std::mem::take(&mut called_by_seat[i]),
                );
                PlayerSnapshot {
                    seat: i as u8,
                    tehai: seat_tehai(&p.hand, melds.len(), our_seat == Some(i as u8), drawing),
                    melds,
                    river,
                    score: p.score,
                    riichi_declared: p.riichi_declared,
                    riichi_stage: p.riichi_stage,
                    double_riichi: p.double_riichi_declared,
                    riichi_declaration_index: p.riichi_declaration_index,
                    kita_tiles: Vec::new(),
                    drawn_tile: if i as u8 == s.current_player {
                        s.drawn_tile.map(tid_to_mjai)
                    } else {
                        None
                    },
                }
            })
            .collect();

        Self {
            bakaze: wind_to_str(s.round_wind).to_string(),
            // riichienv stores kyoku_idx as 0..3 within the round; mjai
            // protocol reports 1..4. Add 1 for parity with start_kyoku
            // events so frontends don't have to remember the offset.
            kyoku: s.kyoku_idx + 1,
            honba: s.honba,
            kyotaku: s.riichi_sticks,
            oya: s.oya,
            current_player: s.current_player,
            turn_count: s.turn_count,
            phase: s.phase.into(),
            is_done: s.is_done,
            num_players: s.players.len() as u8,
            players,
            dora_markers: s
                .wall
                .dora_indicators
                .iter()
                .copied()
                .map(tid_to_mjai)
                .collect(),
            our_seat,
        }
    }

    /// Build a snapshot from a 3-player (sanma) engine state. Mirrors
    /// `from_state` but populates `PlayerSnapshot.kita_tiles` from the 3p
    /// engine's per-player kita pool.
    pub fn from_state_3p(s: &GameState3P, our_seat: Option<u8>) -> Self {
        let in_wait_act = matches!(s.phase, riichienv_core::action::Phase::WaitAct);
        let np = s.players.len();
        let all_melds: Vec<Vec<MeldSnapshot>> = (0..np)
            .map(|i| s.players[i].melds.iter().map(MeldSnapshot::from).collect())
            .collect();
        let mut called_by_seat = called_tiles_by_seat(&all_melds, np);
        let players: Vec<PlayerSnapshot> = all_melds
            .into_iter()
            .enumerate()
            .map(|(i, melds)| {
                let p = &s.players[i];
                let drawing = i as u8 == s.current_player && in_wait_act;
                let river = build_river(
                    &p.discards,
                    &p.discard_from_hand,
                    &p.discard_is_riichi,
                    std::mem::take(&mut called_by_seat[i]),
                );
                PlayerSnapshot {
                    seat: i as u8,
                    tehai: seat_tehai(&p.hand, melds.len(), our_seat == Some(i as u8), drawing),
                    melds,
                    river,
                    score: p.score,
                    riichi_declared: p.riichi_declared,
                    riichi_stage: p.riichi_stage,
                    double_riichi: p.double_riichi_declared,
                    riichi_declaration_index: p.riichi_declaration_index,
                    kita_tiles: p.kita_tiles.iter().copied().map(tid_to_mjai).collect(),
                    drawn_tile: if i as u8 == s.current_player {
                        s.drawn_tile.map(tid_to_mjai)
                    } else {
                        None
                    },
                }
            })
            .collect();

        Self {
            bakaze: wind_to_str(s.round_wind).to_string(),
            kyoku: s.kyoku_idx + 1,
            honba: s.honba,
            kyotaku: s.riichi_sticks,
            oya: s.oya,
            current_player: s.current_player,
            turn_count: s.turn_count,
            phase: s.phase.into(),
            is_done: s.is_done,
            num_players: s.players.len() as u8,
            players,
            dora_markers: s
                .wall
                .dora_indicators
                .iter()
                .copied()
                .map(tid_to_mjai)
                .collect(),
            our_seat,
        }
    }
}

/// Count, per seat, the tiles claimed away from that seat's pond. A meld's
/// `from_who` is the absolute seat the called tile was discarded from, and
/// `called_tile` is that tile; ankan/kakan carry `from_who < 0` and are
/// ignored. The returned map is keyed by mjai tile string with the number of
/// copies of that tile taken from the seat.
fn called_tiles_by_seat(all_melds: &[Vec<MeldSnapshot>], np: usize) -> Vec<HashMap<String, usize>> {
    let mut by_seat: Vec<HashMap<String, usize>> = vec![HashMap::new(); np];
    for melds in all_melds {
        for m in melds {
            if m.from_who >= 0 && (m.from_who as usize) < np {
                if let Some(ct) = &m.called_tile {
                    *by_seat[m.from_who as usize].entry(ct.clone()).or_insert(0) += 1;
                }
            }
        }
    }
    by_seat
}

/// Build one seat's view river, flagging the discards that were claimed away.
/// `called` holds the count of each tile value that left this seat's pond; we
/// flag the first still-unflagged discard matching each value. Matching by
/// value (rather than the exact pond index, which riichienv-core doesn't keep)
/// can pick the wrong copy when the same tile was discarded twice and only one
/// was called — but the copies are visually identical, so the rendered river is
/// unaffected and the hidden count is always exact. The flagged entries stay in
/// the list (the analysis engine still treats them as genbutsu against this
/// seat); only [`crate::game_state::mahgen_view`]'s river encoder skips them.
fn build_river(
    discards: &[u8],
    from_hand: &[bool],
    is_riichi: &[bool],
    mut called: HashMap<String, usize>,
) -> Vec<DiscardEntry> {
    discards
        .iter()
        .enumerate()
        .map(|(idx, &tile)| {
            let t = tid_to_mjai(tile);
            let claimed = match called.get_mut(&t) {
                Some(n) if *n > 0 => {
                    *n -= 1;
                    true
                }
                _ => false,
            };
            DiscardEntry {
                tile: t,
                tedashi: from_hand.get(idx).copied().unwrap_or(true),
                is_riichi: is_riichi.get(idx).copied().unwrap_or(false),
                called: claimed,
            }
        })
        .collect()
}

/// Build the `tehai` for one seat.
///
/// The observer's own hand is maintained correctly by the engine, so its real
/// tiles are used. Every other seat's hand is hidden, and riichienv-core 0.4.8
/// inflates it by one tile per turn: `apply_mjai_event(Tsumo)` pushes the
/// unknown drawn tile, but the following `Dahai` removes by matching the
/// *known* discard — which never matches the unknown tile — so it is never
/// removed. We therefore reconstruct the concealed count from the melds
/// (13 - 3 per called set, +1 while that seat holds a freshly drawn tile) and
/// render it as `"?"` placeholders rather than trusting the engine's `hand`.
fn seat_tehai(hand: &[u8], n_melds: usize, is_observer: bool, drawing: bool) -> Vec<String> {
    if is_observer {
        return hand.iter().copied().map(tid_to_mjai).collect();
    }
    let n = (13 - 3 * n_melds as i32 + drawing as i32).max(0) as usize;
    vec!["?".to_string(); n]
}

fn wind_to_str(w: u8) -> &'static str {
    match w % 4 {
        0 => "E",
        1 => "S",
        2 => "W",
        3 => "N",
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wind_str_covers_all_four() {
        assert_eq!(wind_to_str(0), "E");
        assert_eq!(wind_to_str(1), "S");
        assert_eq!(wind_to_str(2), "W");
        assert_eq!(wind_to_str(3), "N");
        // Wraps cleanly.
        assert_eq!(wind_to_str(4), "E");
    }

    #[test]
    fn meld_type_round_trips_to_kind() {
        assert_eq!(MeldKind::from(MeldType::Chi), MeldKind::Chi);
        assert_eq!(MeldKind::from(MeldType::Pon), MeldKind::Pon);
        assert_eq!(MeldKind::from(MeldType::Daiminkan), MeldKind::Daiminkan);
        assert_eq!(MeldKind::from(MeldType::Ankan), MeldKind::Ankan);
        assert_eq!(MeldKind::from(MeldType::Kakan), MeldKind::Kakan);
    }

    #[test]
    fn snapshot_serializes_with_expected_field_names() {
        // Build a default GameState and assert snapshot keys are stable.
        let rule = riichienv_core::rule::GameRule::default_tenhou();
        let s = GameState::new(0, true, None, 0, rule);
        let snap = GameStateSnapshot::from_state(&s, Some(0));
        let v: serde_json::Value = serde_json::to_value(&snap).unwrap();
        for key in [
            "bakaze",
            "kyoku",
            "honba",
            "kyotaku",
            "oya",
            "current_player",
            "turn_count",
            "phase",
            "is_done",
            "players",
            "dora_markers",
        ] {
            assert!(v.get(key).is_some(), "missing key: {key}");
        }
        // Initial state: 4 players present.
        assert_eq!(v["players"].as_array().unwrap().len(), 4);
    }
}
