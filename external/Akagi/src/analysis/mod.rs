//! Analysis engine — Rust port of the mahjong-helper Go `util/` package.
//!
//! Phase 1 (current): tiles, hand input, shanten, per-tile waits.
//! Later phases add Hand13/Hand14 search, agari rate, point expectation,
//! and the risk engine. See `README.md` in this directory for the
//! contributor-facing layout.
//!
//! ## License note
//!
//! Algorithms and numerical tables are facts and are not subject to
//! copyright. The mahjong-helper Go source is consulted for behaviour
//! but not copied verbatim. See `README.md`.

pub mod agari_rate;
pub mod data;
pub mod hand;
pub mod improves;
pub mod result;
pub mod risk;
pub mod runner;
pub mod score;
pub mod search;
pub mod shanten;
pub mod snapshot_adapter;
pub mod tenpai_rate;
pub mod tile;
pub mod waits;

pub use hand::{Counts34, Meld34, Meld34Kind, OpponentInfo, PlayerInfo34};
pub use improves::analyze_13;
pub use result::{
    AnalysisResult, DiscardCandidate, Hand13Result, Hand14Result, HandState, OpponentRisk, WaitInfo,
};
pub use risk::RiskVec;
pub use search::analyze_14;
pub use shanten::{shanten, Shanten};
pub use tile::{Tile34, TILE_COUNT};
pub use waits::{waits, Waits};

/// Top-level dispatch.
///
/// 13-tile state → `Hand13Result` populated, `hand14 = None`.
/// 14-tile state → `Hand14Result` populated, `hand13 = None`.
/// In both cases opponents + mixed risk + best defence discard are filled in.
pub fn analyze(info: &PlayerInfo34) -> AnalysisResult {
    let s = shanten::shanten(info);
    let (state, hand13, hand14) = if info.is_drawing_state() {
        (HandState::Wait13, Some(analyze_13(info)), None)
    } else {
        (HandState::Discard14, None, Some(analyze_14(info)))
    };

    // Opponent risk fan-out.
    let opp_vecs: Vec<risk::OpponentRiskVec> = info
        .opponents
        .iter()
        .map(|op| risk::for_opponent(info, op))
        .collect();
    let mixed = risk::mixed_risk(&opp_vecs);

    let opponents: Vec<OpponentRisk> = opp_vecs
        .iter()
        .zip(info.opponents.iter())
        .map(|(v, src)| OpponentRisk {
            seat: v.seat,
            tenpai_rate: v.tenpai_rate,
            risk: v.risk.to_vec(),
            is_riichi: src.is_riichi,
        })
        .collect();

    let best_attack = hand14
        .as_ref()
        .and_then(|h| h.maintain.first())
        .map(|c| c.discard.clone());
    let best_defence_idx = risk::best_defence(&info.hand, &mixed);
    let best_defence = best_defence_idx.map(|i| Tile34(i).to_mjai().to_string());

    AnalysisResult {
        seat: info.seat,
        turn: info.turn,
        shanten: s,
        state,
        hand13,
        hand14,
        opponents,
        mixed_risk: mixed.to_vec(),
        best_attack_discard: best_attack,
        best_defence_discard: best_defence,
    }
}
