//! Live inference engine: maintains a riichienv-core game state from mjai
//! events and, at our decision points, runs the CNN to pick a **legal** action.
//!
//! Transport-agnostic: it consumes `riichienv_core::replay::MjaiEvent` and
//! returns a schema-agnostic [`BotAction`] (mjai tile strings), which the
//! Akagi-side `NativeBot` maps to Akagi's own mjai event type. This keeps
//! `native_bot` free of any Akagi dependency.
//!
//! `decide()` is read-only with respect to game *state*: our own chosen action
//! is NOT applied here. In Akagi, every action (ours included) echoes back
//! through the mjai bus and is applied via [`Engine::feed`], so applying it in
//! `decide()` too would double-count.

use anyhow::Result;
use riichienv_core::action::{Action, ActionType};
use riichienv_core::parser::tid_to_mjai;
use riichienv_core::replay::MjaiEvent;
use riichienv_core::rule::GameRule;
use riichienv_core::state::GameState;
use riichienv_core::state_3p::GameState3P;

use crate::action_codec::{pick_by_logits, rank_by_logits};
use crate::adapt::{obs_and_legal_3p, obs_and_legal_4p};
use crate::mjai_compat::{parse_line, sanitize_3p};
use crate::model::{ExternalActivateFn, ExternalInferenceFn, Model};
use crate::mortal::{
    active_contract, encode_mortal_obs_3p, encode_mortal_obs_4p, rank_by_mortal_logits,
    CustomContract,
};

/// How many ranked candidates the engine surfaces for the HUD's multi-row
/// recommendation card (top-N by policy probability).
const SHOW_TOP_N: usize = 3;

/// A schema-agnostic bot reply, ready to be mapped to Akagi's `MjaiEvent`.
/// All tiles are mjai strings (e.g. `"5mr"`, `"P"`).
#[derive(Debug, Clone, PartialEq)]
pub enum BotAction {
    Dahai {
        pai: String,
        tsumogiri: bool,
    },
    /// Riichi declaration; `pai` is the predicted riichi discard (mjai must
    /// carry it or autoplay stalls). Always non-empty on [`Decision::action`] —
    /// `decide` drops a riichi it can't name a discard for. Runner-up
    /// `candidates` rows carry an empty `pai`: they only decorate the HUD, and
    /// predicting a discard for each would run the model once per row.
    Reach {
        pai: String,
    },
    Pon {
        target: u8,
        pai: String,
        consumed: Vec<String>,
    },
    Chi {
        target: u8,
        pai: String,
        consumed: Vec<String>,
    },
    Daiminkan {
        target: u8,
        pai: String,
        consumed: Vec<String>,
    },
    Ankan {
        consumed: Vec<String>,
    },
    Kakan {
        pai: String,
        consumed: Vec<String>,
    },
    /// Ron or tsumo (both are mjai `hora`); `target` is the loser (self for
    /// tsumo, the discarder for ron).
    Hora {
        target: u8,
    },
    /// Nine-terminals abortive draw (mjai `ryukyoku`).
    Kyushu,
    /// Kita / nukidora (sanma).
    Kita,
    /// No action this turn.
    Pass,
}

/// Our seat's decision at a given point.
pub struct Decision {
    pub action: BotAction,
    /// Top legal actions ranked by policy probability (best first). `action`
    /// is `candidates[0].0`; the rest are the runner-up recommendations the HUD
    /// shows as a top-N card. Probabilities are a softmax over the legal set.
    pub candidates: Vec<(BotAction, f32)>,
    /// Raw action logits (indexed by the mode's action space).
    pub logits: Vec<f32>,
    /// The full legal set (before the top-N cut in `candidates`) held exactly
    /// one action, so this "decision" is forced — any policy, local or remote,
    /// can only play `action`. Callers that pay per query (the online API) use
    /// this to answer locally instead of spending a call on a foregone move.
    pub forced: bool,
    /// The requested custom policy failed and this decision used bundled weights.
    pub custom_fallback: bool,
}

enum Backend {
    Four {
        state: Box<GameState>,
        model: Model,
    },
    Three {
        state: Box<GameState3P>,
        model: Model,
    },
}

pub struct Engine {
    backend: Backend,
    seat: u8,
    num_players: u8,
}

impl Engine {
    /// Construct for `num_players`, loading weights from a safetensors buffer.
    pub fn new(model_bytes: Vec<u8>, num_players: u8, seat: u8) -> Result<Self> {
        let rule = GameRule::default_tenhou();
        let model = Model::from_safetensors(model_bytes, num_players)?;
        Self::with_model(model, num_players, seat, rule)
    }

    pub fn new_with_external(model_bytes: Vec<u8>, num_players: u8, seat: u8,
        callback: ExternalInferenceFn, activate: ExternalActivateFn) -> Result<Self> {
        let rule = GameRule::default_tenhou();
        let model = Model::from_safetensors_with_external(model_bytes, num_players, callback, activate)?;
        Self::with_model(model, num_players, seat, rule)
    }

    fn with_model(model: Model, num_players: u8, seat: u8, rule: GameRule) -> Result<Self> {
        let backend = if num_players == 3 {
            Backend::Three {
                // Keep mjai logging on: it is the only place riichienv records the
                // per-discard tsumogiri bit, which the observation encoder needs.
                state: Box::new(GameState3P::new(0, false, None, 0, rule)),
                model,
            }
        } else {
            Backend::Four {
                state: Box::new(GameState::new(0, true, None, 0, rule)),
                model,
            }
        };
        Ok(Self {
            backend,
            seat,
            num_players,
        })
    }

    /// Reset to a fresh game while keeping the loaded weights.
    pub fn reset(&mut self) {
        let rule = GameRule::default_tenhou();
        match &mut self.backend {
            Backend::Four { state, .. } => {
                *state = Box::new(GameState::new(0, true, None, 0, rule))
            }
            Backend::Three { state, .. } => {
                *state = Box::new(GameState3P::new(0, false, None, 0, rule))
            }
        }
    }

    pub fn seat(&self) -> u8 {
        self.seat
    }

    pub fn set_seat(&mut self, seat: u8) {
        self.seat = seat;
    }

    pub fn num_players(&self) -> u8 {
        self.num_players
    }

    pub fn feed(&mut self, ev: MjaiEvent) {
        let opponent_kakan = match &ev {
            MjaiEvent::Kakan { actor, pai } if *actor as u8 != self.seat => {
                Some((*actor as u8, pai.clone()))
            }
            _ => None,
        };
        match &mut self.backend {
            Backend::Four { state, .. } => {
                state.apply_mjai_event(ev);
                if let Some((actor, pai)) = opponent_kakan {
                    crate::chankan::open_on_kakan(state, actor, &pai, self.seat);
                }
            }
            Backend::Three { state, .. } => {
                let mut ev = ev;
                sanitize_3p(&mut ev);
                crate::mjai_compat::apply_sanma_event(state, ev);
                if let Some((actor, pai)) = opponent_kakan {
                    crate::chankan::open_on_kakan_3p(state, actor, &pai, self.seat);
                }
            }
        }
    }

    pub fn feed_line(&mut self, line: &str) -> bool {
        match parse_line(line, self.num_players) {
            Some(ev) => {
                self.feed(ev);
                true
            }
            None => false,
        }
    }

    /// Decide our action at the current state. `None` if we currently have no
    /// legal action (not our turn / nothing to respond to).
    pub fn decide(&mut self) -> Result<Option<Decision>> {
        let seat = self.seat;
        let contract = active_contract(self.num_players);
        let (mut ranked, logits, last_discarder, drawn, reach_pai, forced) = match &mut self.backend
        {
            Backend::Four { state, model } => {
                let last_discarder = state.last_discard.map(|(pid, _tile)| pid);
                let drawn = state.drawn_tile;

                if model.uses_external() {
                    if let Some((ranked, logits, reach_pai, forced)) =
                        decide_mortal_4p(state, model, seat, &contract)
                    {
                        (ranked, logits, last_discarder, drawn, reach_pai, forced)
                    } else {
                        let (obs, legal) = obs_and_legal_4p(state, seat);
                        if legal.is_empty() {
                            return Ok(None);
                        }
                        let forced = legal.len() == 1;
                        let logits = match &contract {
                            CustomContract::AkagiOnnx => model.forward_logits(&obs)?,
                            _ => model.forward_bundled_logits(&obs)?,
                        };
                        let ranked = rank_by_logits(&legal, &logits, 4, SHOW_TOP_N);
                        let Some((top, _)) = ranked.first() else {
                            return Ok(None);
                        };
                        let reach_pai = if top.action_type == ActionType::Riichi {
                            predict_reach_discard(state, model, seat, 4)
                        } else {
                            None
                        };
                        (ranked, logits, last_discarder, drawn, reach_pai, forced)
                    }
                } else {
                    let (obs, legal) = obs_and_legal_4p(state, seat);
                    if legal.is_empty() {
                        return Ok(None);
                    }
                    let forced = legal.len() == 1;
                    let logits = model.forward_logits(&obs)?;
                    let ranked = rank_by_logits(&legal, &logits, 4, SHOW_TOP_N);
                    let Some((top, _)) = ranked.first() else {
                        return Ok(None);
                    };
                    let reach_pai = if top.action_type == ActionType::Riichi {
                        predict_reach_discard(state, model, seat, 4)
                    } else {
                        None
                    };
                    (ranked, logits, last_discarder, drawn, reach_pai, forced)
                }
            }
            Backend::Three { state, model } => {
                let last_discarder = state.last_discard.map(|(pid, _tile)| pid);
                let drawn = state.drawn_tile;

                if model.uses_external() {
                    if let Some((ranked, logits, reach_pai, forced)) =
                        decide_mortal_3p(state, model, seat, &contract)
                    {
                        (ranked, logits, last_discarder, drawn, reach_pai, forced)
                    } else {
                        let (obs, legal) = obs_and_legal_3p(state, seat);
                        if legal.is_empty() {
                            return Ok(None);
                        }
                        let forced = legal.len() == 1;
                        let logits = match &contract {
                            CustomContract::AkagiOnnx => model.forward_logits(&obs)?,
                            _ => model.forward_bundled_logits(&obs)?,
                        };
                        let ranked = rank_by_logits(&legal, &logits, 3, SHOW_TOP_N);
                        let Some((top, _)) = ranked.first() else {
                            return Ok(None);
                        };
                        let reach_pai = if top.action_type == ActionType::Riichi {
                            predict_reach_discard_3p(state, model, seat)
                        } else {
                            None
                        };
                        (ranked, logits, last_discarder, drawn, reach_pai, forced)
                    }
                } else {
                    let (obs, legal) = obs_and_legal_3p(state, seat);
                    if legal.is_empty() {
                        return Ok(None);
                    }
                    let forced = legal.len() == 1;
                    let logits = model.forward_logits(&obs)?;
                    let ranked = rank_by_logits(&legal, &logits, 3, SHOW_TOP_N);
                    let Some((top, _)) = ranked.first() else {
                        return Ok(None);
                    };
                    let reach_pai = if top.action_type == ActionType::Riichi {
                        predict_reach_discard_3p(state, model, seat)
                    } else {
                        None
                    };
                    (ranked, logits, last_discarder, drawn, reach_pai, forced)
                }
            }
        };

        if reach_pai.is_none()
            && matches!(ranked.first(), Some((a, _)) if a.action_type == ActionType::Riichi)
        {
            ranked.retain(|(a, _)| a.action_type != ActionType::Riichi);
        }
        if ranked.is_empty() {
            return Ok(None);
        }

        let candidates = build_candidates(&ranked, seat, last_discarder, drawn, reach_pai);
        let action = candidates[0].0.clone();
        let custom_fallback = match &self.backend {
            Backend::Four { model, .. } | Backend::Three { model, .. } => model.take_external_failure(),
        };
        Ok(Some(Decision {
            action,
            candidates,
            logits,
            forced,
            custom_fallback,
        }))
    }

    pub fn uses_external_policy(&self) -> bool {
        match &self.backend {
            Backend::Four { model, .. } | Backend::Three { model, .. } => model.uses_external(),
        }
    }

    pub fn uses_experimental_policy(&self) -> bool {
        self.uses_external_policy() && !matches!(active_contract(self.num_players), CustomContract::AkagiOnnx)
    }

    /// Return a human-readable description of the active model for the HUD.
    pub fn active_model_label(&self) -> &'static str {
        if !self.uses_external_policy() {
            return "Akagi 内置策略";
        }
        match active_contract(self.num_players) {
            CustomContract::AkagiOnnx => "自定义 ONNX 策略",
            CustomContract::MortalOnnx(_) => "自定义 Mortal (ONNX)",
            CustomContract::MortalPth(_) => "自定义 Mortal (PTH)",
        }
    }

    pub fn reach_discard(&mut self) -> Option<String> {
        let seat = self.seat;
        match &mut self.backend {
            Backend::Four { state, model } => {
                predict_reach_discard(state, model, seat, 4).map(tid_to_mjai)
            }
            Backend::Three { state, model } => {
                predict_reach_discard_3p(state, model, seat).map(tid_to_mjai)
            }
        }
    }
}

fn eval_mortal_logits(
    obs: &[f32],
    model: &Model,
    contract: &CustomContract,
    legal: &[Action],
    players: u8,
) -> Option<Vec<f32>> {
    match contract {
        CustomContract::MortalPth(pth) => match pth.forward_logits(obs) {
            Ok(logits) if logits.iter().all(|v| v.is_finite()) => Some(logits),
            _ => {
                model.mark_external_failed();
                None
            }
        },
        CustomContract::MortalOnnx(meta) => {
            // The host receives a real legal mask appended after the observation.
            let mut input = obs.to_vec();
            let mut mask = vec![0.0; meta.action_space];
            for action in legal {
                if let Some(i) = crate::mortal::mortal_action_index(action, players) {
                    if i < mask.len() { mask[i] = 1.0; }
                }
            }
            input.extend(mask);
            model.forward_external_custom(&input, meta.action_space)
        },
        CustomContract::AkagiOnnx => None,
    }
}

fn decide_mortal_4p(
    state: &mut GameState,
    model: &Model,
    seat: u8,
    contract: &CustomContract,
) -> Option<(Vec<(Action, f32)>, Vec<f32>, Option<u8>, bool)> {
    let in_channels = match contract {
        CustomContract::MortalPth(pth) => pth.meta.in_channels,
        CustomContract::MortalOnnx(meta) => meta.in_channels,
        CustomContract::AkagiOnnx => return None,
    };
    let (obs, legal) = encode_mortal_obs_4p(state, seat, in_channels);
    if legal.is_empty() {
        return None;
    }
    let forced = legal.len() == 1;
    let logits = eval_mortal_logits(&obs, model, contract, &legal, 4)?;
    let ranked = rank_by_mortal_logits(&legal, &logits, 4, SHOW_TOP_N);
    let (top, _) = ranked.first()?;
    let reach_pai = if top.action_type == ActionType::Riichi {
        let mut clone = state.clone();
        clone.apply_mjai_event(MjaiEvent::Reach {
            actor: seat as usize,
        });
        let (obs_r, legal_r) = encode_mortal_obs_4p(&mut clone, seat, in_channels);
        if legal_r.is_empty() {
            None
        } else {
            let logits_r = eval_mortal_logits(&obs_r, model, contract, &legal_r, 4)?;
            rank_by_mortal_logits(&legal_r, &logits_r, 4, 1)
                .into_iter()
                .next()
                .and_then(|(a, _)| a.tile)
        }
    } else {
        None
    };
    Some((ranked, logits, reach_pai, forced))
}

fn decide_mortal_3p(
    state: &mut GameState3P,
    model: &Model,
    seat: u8,
    contract: &CustomContract,
) -> Option<(Vec<(Action, f32)>, Vec<f32>, Option<u8>, bool)> {
    let in_channels = match contract {
        CustomContract::MortalPth(pth) => pth.meta.in_channels,
        CustomContract::MortalOnnx(meta) => meta.in_channels,
        CustomContract::AkagiOnnx => return None,
    };
    let (obs, legal) = encode_mortal_obs_3p(state, seat, in_channels);
    if legal.is_empty() {
        return None;
    }
    let forced = legal.len() == 1;
    let logits = eval_mortal_logits(&obs, model, contract, &legal, 3)?;
    let ranked = rank_by_mortal_logits(&legal, &logits, 3, SHOW_TOP_N);
    let (top, _) = ranked.first()?;
    let reach_pai = if top.action_type == ActionType::Riichi {
        let mut clone = state.clone();
        clone.apply_mjai_event(MjaiEvent::Reach {
            actor: seat as usize,
        });
        let (obs_r, legal_r) = encode_mortal_obs_3p(&mut clone, seat, in_channels);
        if legal_r.is_empty() {
            None
        } else {
            let logits_r = eval_mortal_logits(&obs_r, model, contract, &legal_r, 3)?;
            rank_by_mortal_logits(&legal_r, &logits_r, 3, 1)
                .into_iter()
                .next()
                .and_then(|(a, _)| a.tile)
        }
    } else {
        None
    };
    Some((ranked, logits, reach_pai, forced))
}

fn build_candidates(
    ranked: &[(Action, f32)],
    seat: u8,
    last_discarder: Option<u8>,
    drawn: Option<u8>,
    reach_pai: Option<u8>,
) -> Vec<(BotAction, f32)> {
    ranked
        .iter()
        .enumerate()
        .map(|(i, (a, p))| {
            let rp = if i == 0 { reach_pai } else { None };
            (build_bot_action(a, seat, last_discarder, drawn, rp), *p)
        })
        .collect()
}

fn predict_reach_discard(state: &GameState, model: &Model, seat: u8, np: u8) -> Option<u8> {
    let mut clone = state.clone();
    clone.apply_mjai_event(MjaiEvent::Reach {
        actor: seat as usize,
    });
    let (obs, legal) = obs_and_legal_4p(&mut clone, seat);
    if legal.is_empty() {
        return None;
    }
    let logits = model.forward_logits(&obs).ok()?;
    let a = pick_by_logits(&legal, &logits, np)?;
    a.tile
}

fn predict_reach_discard_3p(state: &GameState3P, model: &Model, seat: u8) -> Option<u8> {
    let mut clone = state.clone();
    clone.apply_mjai_event(MjaiEvent::Reach {
        actor: seat as usize,
    });
    let (obs, legal) = obs_and_legal_3p(&mut clone, seat);
    if legal.is_empty() {
        return None;
    }
    let logits = model.forward_logits(&obs).ok()?;
    let a = pick_by_logits(&legal, &logits, 3)?;
    a.tile
}

fn build_bot_action(
    a: &Action,
    seat: u8,
    last_discarder: Option<u8>,
    drawn: Option<u8>,
    reach_pai: Option<u8>,
) -> BotAction {
    let consumed = |a: &Action| a.consume_tiles.iter().map(|&t| tid_to_mjai(t)).collect();
    let target = last_discarder.unwrap_or(0);
    match a.action_type {
        ActionType::Discard => match a.tile {
            Some(t) => BotAction::Dahai {
                pai: tid_to_mjai(t),
                tsumogiri: drawn == Some(t),
            },
            None => BotAction::Pass,
        },
        ActionType::Riichi => BotAction::Reach {
            pai: reach_pai.map(tid_to_mjai).unwrap_or_default(),
        },
        ActionType::Pon => BotAction::Pon {
            target,
            pai: a.tile.map(tid_to_mjai).unwrap_or_default(),
            consumed: consumed(a),
        },
        ActionType::Chi => BotAction::Chi {
            target,
            pai: a.tile.map(tid_to_mjai).unwrap_or_default(),
            consumed: consumed(a),
        },
        ActionType::Daiminkan => BotAction::Daiminkan {
            target,
            pai: a.tile.map(tid_to_mjai).unwrap_or_default(),
            consumed: consumed(a),
        },
        ActionType::Ankan => BotAction::Ankan {
            consumed: consumed(a),
        },
        ActionType::Kakan => BotAction::Kakan {
            pai: a.tile.map(tid_to_mjai).unwrap_or_default(),
            consumed: consumed(a),
        },
        ActionType::Tsumo => BotAction::Hora { target: seat },
        ActionType::Ron => BotAction::Hora {
            target: last_discarder.unwrap_or(seat),
        },
        ActionType::KyushuKyuhai => BotAction::Kyushu,
        ActionType::Kita => BotAction::Kita,
        ActionType::Pass => BotAction::Pass,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riichienv_core::replay::MjaiEvent;

    fn ev(line: &str) -> MjaiEvent {
        serde_json::from_str(line).expect("valid mjai event")
    }

    fn act(kind: ActionType, tile: Option<u8>, consumed: Vec<u8>) -> Action {
        Action::new(kind, tile, consumed, Some(0))
    }

    #[test]
    fn claims_and_ron_target_the_discarder_seat() {
        let discarder = Some(3u8);
        let seat = 0u8;

        let pon = build_bot_action(
            &act(ActionType::Pon, Some(4), vec![5, 6]),
            seat,
            discarder,
            None,
            None,
        );
        assert_eq!(
            pon,
            BotAction::Pon {
                target: 3,
                pai: "2m".into(),
                consumed: vec!["2m".into(), "2m".into()],
            }
        );

        let chi = build_bot_action(
            &act(ActionType::Chi, Some(4), vec![8, 12]),
            seat,
            discarder,
            None,
            None,
        );
        assert!(matches!(chi, BotAction::Chi { target: 3, .. }));

        let kan = build_bot_action(
            &act(ActionType::Daiminkan, Some(4), vec![5, 6, 7]),
            seat,
            discarder,
            None,
            None,
        );
        assert!(matches!(kan, BotAction::Daiminkan { target: 3, .. }));

        let ron = build_bot_action(
            &act(ActionType::Ron, None, vec![]),
            seat,
            discarder,
            None,
            None,
        );
        assert_eq!(ron, BotAction::Hora { target: 3 });
    }

    #[test]
    fn tsumo_targets_self() {
        let seat = 2u8;
        let tsumo = build_bot_action(
            &act(ActionType::Tsumo, None, vec![]),
            seat,
            Some(1),
            None,
            None,
        );
        assert_eq!(tsumo, BotAction::Hora { target: seat });

        let ron_no_discarder =
            build_bot_action(&act(ActionType::Ron, None, vec![]), seat, None, None, None);
        assert_eq!(ron_no_discarder, BotAction::Hora { target: seat });
    }

    #[test]
    fn discard_marks_tsumogiri_only_for_the_drawn_tile() {
        let drawn = Some(88u8); // 5sr
        let a = act(ActionType::Discard, Some(88), vec![]);
        assert_eq!(
            build_bot_action(&a, 0, None, drawn, None),
            BotAction::Dahai {
                pai: "5sr".into(),
                tsumogiri: true
            }
        );
        let a = act(ActionType::Discard, Some(0), vec![]);
        assert_eq!(
            build_bot_action(&a, 0, None, drawn, None),
            BotAction::Dahai {
                pai: "1m".into(),
                tsumogiri: false
            }
        );
    }

    #[test]
    fn closed_calls_and_terminals_map_without_a_target() {
        assert_eq!(
            build_bot_action(
                &act(ActionType::Ankan, None, vec![0, 1, 2, 3]),
                0,
                Some(2),
                None,
                None
            ),
            BotAction::Ankan {
                consumed: vec!["1m".into(), "1m".into(), "1m".into(), "1m".into()],
            }
        );
        assert!(matches!(
            build_bot_action(
                &act(ActionType::Kakan, Some(0), vec![1]),
                0,
                Some(2),
                None,
                None
            ),
            BotAction::Kakan { .. }
        ));
        assert_eq!(
            build_bot_action(
                &act(ActionType::KyushuKyuhai, None, vec![]),
                0,
                None,
                None,
                None
            ),
            BotAction::Kyushu
        );
        assert_eq!(
            build_bot_action(&act(ActionType::Kita, None, vec![]), 0, None, None, None),
            BotAction::Kita
        );
        assert_eq!(
            build_bot_action(&act(ActionType::Pass, None, vec![]), 0, None, None, None),
            BotAction::Pass
        );
    }

    #[test]
    fn riichi_carries_the_predicted_discard() {
        let a = act(ActionType::Riichi, None, vec![]);
        assert_eq!(
            build_bot_action(&a, 0, None, None, Some(36)),
            BotAction::Reach { pai: "1p".into() }
        );
    }

    #[test]
    fn claim_target_is_a_seat_not_a_tile_id() {
        let ours = r#"["9s","9s","1m","2m","3m","4m","5m","6m","7m","8m","9m","1p","2p"]"#;
        let other = r#"["1m","2m","3m","4m","5m","6m","7m","8m","9m","1p","2p","3p","4p"]"#;
        let mut eng = crate::defaults::engine(4, 0).expect("bundled 4p weights");
        eng.feed(ev(r#"{"type":"start_game","names":["a","b","c","d"]}"#));
        eng.feed(ev(&format!(
            r#"{{"type":"start_kyoku","bakaze":"E","dora_marker":"2m","kyoku":1,"honba":0,
                 "kyotaku":0,"oya":0,"scores":[25000,25000,25000,25000],
                 "tehais":[{ours},{other},{other},{other}]}}"#
        )));
        eng.feed(ev(r#"{"type":"tsumo","actor":1,"pai":"9s"}"#));
        eng.feed(ev(
            r#"{"type":"dahai","actor":1,"pai":"9s","tsumogiri":true}"#,
        ));

        let d = eng
            .decide()
            .expect("forward pass")
            .expect("a pon window is a decision point");

        let mut saw_pon = false;
        for (a, _) in &d.candidates {
            match a {
                BotAction::Pon { target, .. }
                | BotAction::Chi { target, .. }
                | BotAction::Daiminkan { target, .. }
                | BotAction::Hora { target } => {
                    assert_eq!(
                        *target, 1,
                        "target must be the discarder's seat, got {target}"
                    );
                    saw_pon |= matches!(a, BotAction::Pon { .. });
                }
                _ => {}
            }
        }
        assert!(
            saw_pon,
            "fixture invalid: no pon offered on the 9s discard ({:?})",
            d.candidates.iter().map(|(a, _)| a).collect::<Vec<_>>()
        );
    }

    #[test]
    fn feed_line_applies_the_nukidora_rename() {
        let h = r#"["1m","9m","1p","2p","3p","4p","5p","6p","7p","8p","9p","N","N"]"#;
        let mut eng = crate::defaults::engine(3, 0).expect("bundled 3p weights");
        assert!(eng.feed_line(r#"{"type":"start_game","names":["a","b","c",""]}"#));
        assert!(eng.feed_line(&format!(
            r#"{{"type":"start_kyoku","bakaze":"E","dora_marker":"1s","kyoku":1,"honba":0,
                 "kyotaku":0,"oya":0,"scores":[35000,35000,35000,0],
                 "tehais":[{h},{h},{h},{h}]}}"#
        )));
        assert!(eng.feed_line(r#"{"type":"tsumo","actor":0,"pai":"9p"}"#));
        assert!(
            eng.feed_line(r#"{"type":"nukidora","actor":0}"#),
            "a nukidora must reach the engine, not be skipped as an unknown type"
        );

        assert!(!eng.feed_line(""));
        assert!(!eng.feed_line("{oops"));
        assert!(!eng.feed_line(r#"{"type":"some_future_event"}"#));
    }

    #[test]
    fn feed_sanitizes_four_seat_sanma_start_kyoku() {
        let h = r#"["1m","9m","1p","2p","3p","4p","5p","6p","7p","8p","9p","1s","2s"]"#;
        let mut eng = crate::defaults::engine(3, 0).expect("bundled 3p weights");
        eng.feed(ev(r#"{"type":"start_game","names":["a","b","c","d"]}"#));
        eng.feed(ev(&format!(
            r#"{{"type":"start_kyoku","bakaze":"E","dora_marker":"1s","kyoku":1,"honba":0,
                 "kyotaku":0,"oya":0,"scores":[35000,35000,35000,0],
                 "tehais":[{h},{h},{h},{h}]}}"#
        )));
        eng.feed(ev(r#"{"type":"tsumo","actor":0,"pai":"9p"}"#));
        let d = eng.decide().expect("forward pass");
        assert!(d.is_some(), "our own draw is a decision point");
    }
}
