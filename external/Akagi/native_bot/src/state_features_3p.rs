//! 3-player (sanma) variants of the seat view.
//!
//! Mirrors the 4-player builder in this module but reads `GameState3P`. All
//! collections stay 4-wide so the shared encoder can index them; only the first
//! `num_players` entries are consumed, and the encoder is told the real count
//! through `Layout::num_players`.

use riichienv_core::state_3p::GameState3P;

use crate::mortal_obs::{build_kawa, mortal_obs_deaka, mortal_obs_is_aka, KawaItem};
use crate::state_features::MortalSeatView;

/// Players in a sanma hand.
pub const NUM_PLAYERS_3P: usize = 3;

const T: usize = crate::mortal_obs::TILES;

/// Self-relative index for a sanma seat.
fn rel3(pid: u8, seat: u8) -> usize {
    ((pid + NUM_PLAYERS_3P as u8 - seat) as usize) % NUM_PLAYERS_3P
}

/// Shanten for a concealed-hand count vector.
///
/// `tehai_len_div3` is the hand *length* divided by three (4 for a full
/// 13/14-tile hand), not the number of called melds - `calc_shanten_from_counts`
/// uses it to decide whether chiitoitsu/kokushi are worth testing.
///
/// Callers pass the **13-tile** hand: libriichi computes shanten before the drawn
/// tile is folded in, so a 14-tile vector must be reduced first. See
/// [`shanten_from_hand_vec`].
fn shanten_3p(tehai: &[u8; T]) -> u8 {
    let len: usize = tehai.iter().map(|&c| c as usize).sum();
    let s = riichienv_core::shanten::calc_shanten_from_counts_3p(tehai, (len / 3) as u8);
    s.clamp(0, 6) as u8
}

/// Shanten of a raw hand vector (tile ids 0..135) with the drawn tile removed.
///
/// libriichi's `PlayerState.shanten` is maintained from the 3n+1 hand, i.e. it
/// excludes the freshly drawn tile. Computing it on the 3n+2 hand can read one
/// lower (the drawn tile completes a set), which is what the sanma parity
/// fixture showed on two of its eight cases.
fn shanten_from_hand_vec(hand: &[u8], drawn: Option<u8>) -> u8 {
    let mut counts = [0u8; T];
    for &t in hand {
        counts[mortal_obs_deaka(t)] += 1;
    }
    if let Some(d) = drawn {
        let i = mortal_obs_deaka(d);
        if counts[i] > 0 {
            counts[i] -= 1;
        }
    }
    shanten_3p(&counts)
}

/// Per-discard tedashi flags for every seat, reconstructed from the state's
/// mjai log.
///
/// riichienv's 3-player `Dahai` handler does not push to `discard_from_hand`
/// (unlike the 4-player one) and never populates `Observation3P::tsumogiri_flags`,
/// so the only place the tsumogiri bit survives is the raw mjai log. The engine
/// therefore runs the sanma state with logging enabled.
///
/// Returns `None` when the log is unavailable, in which case callers fall back
/// to treating discards as tedashi-free.
fn tedashi_from_log(state: &GameState3P) -> Option<[Vec<bool>; NUM_PLAYERS_3P]> {
    if state.mjai_log.is_empty() {
        return None;
    }
    let mut flags: [Vec<bool>; NUM_PLAYERS_3P] = Default::default();
    for line in &state.mjai_log {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("dahai") {
            continue;
        }
        let Some(actor) = v.get("actor").and_then(|a| a.as_u64()) else {
            continue;
        };
        let tsumogiri = v.get("tsumogiri").and_then(|t| t.as_bool()).unwrap_or(false);
        if let Some(slot) = flags.get_mut(actor as usize) {
            slot.push(!tsumogiri);
        }
    }
    Some(flags)
}

/// Tiles this seat has seen.
///
/// libriichi's `tiles_seen` counts the seat's own hand plus everything visible
/// on the table (discards, melds, dora indicators). In sanma the manzu 2-8
/// tiles are not used at all, and the reference marks all four copies of each
/// as seen, so they are added here too.
fn tiles_seen_3p(state: &GameState3P, seat: u8, dora_indicators: &[u8]) -> [u8; T] {
    let mut seen = [0u8; T];
    // Dead manzu: 2m..=8m are tile indices 1..=7.
    for i in 1..=7usize {
        seen[i] = 4;
    }

    let me = &state.players[seat as usize];
    for &t in &me.hand {
        let i = mortal_obs_deaka(t);
        seen[i] = seen[i].saturating_add(1);
    }
    for player in state.players.iter() {
        for &t in player.discards.iter() {
            let i = mortal_obs_deaka(t);
            seen[i] = seen[i].saturating_add(1);
        }
        for m in &player.melds {
            for &t in &m.tiles {
                let i = mortal_obs_deaka(t);
                seen[i] = seen[i].saturating_add(1);
            }
        }
        for &t in &player.kita_tiles {
            seen[mortal_obs_deaka(t)] = seen[mortal_obs_deaka(t)].saturating_add(1);
        }
    }
    // The replay dependency keeps the called discard in its original pond.
    // Count that physical tile once, even though it also appears in the meld.
    for player in &state.players {
        for meld in &player.melds {
            if meld.opened {
                if let Some(tile) = meld.called_tile {
                    let t = mortal_obs_deaka(tile);
                    seen[t] = seen[t].saturating_sub(1);
                }
            }
        }
    }
    for &t in dora_indicators {
        let i = mortal_obs_deaka(t);
        seen[i] = seen[i].saturating_add(1);
    }
    seen
}

/// How many dora each seat holds (hand + melds).
///
/// A tile counts once per indicator that makes it a dora, and red fives always
/// count.
fn doras_owned_3p(state: &GameState3P, seat: u8, dora_indicators: &[u8]) -> [u8; 4] {
    let mut owned = [0u8; 4];
    let dora34: Vec<usize> = dora_indicators
        .iter()
        .map(|&t| crate::tiles::next_dora_tile34(mortal_obs_deaka(t) as u8, 3) as usize)
        .collect();

    for (pid, player) in state.players.iter().enumerate() {
        let mut count = 0u8;
        let mut tally = |t: u8| {
            let t34 = mortal_obs_deaka(t);
            for &d in &dora34 {
                if d == t34 {
                    count = count.saturating_add(1);
                }
            }
            if mortal_obs_is_aka(t) {
                count = count.saturating_add(1);
            }
        };
        if pid == seat as usize {
            for &t in &player.hand { tally(t); }
        }
        for m in &player.melds {
            for &t in &m.tiles {
                tally(t);
            }
        }
        owned[pid] = count;
    }
    owned
}

impl MortalSeatView {
    /// Derive the view for `seat` from a live 3-player game state.
    ///
    /// Fills the structural fields only; the legal-action context is added by
    /// [`Self::with_legal_actions_3p`].
    pub fn from_state_3p(state: &GameState3P, seat: u8) -> Self {
        let mut v = MortalSeatView {
            n_players: NUM_PLAYERS_3P as u8,
            honba: state.honba,
            kyotaku: state.riichi_sticks,
            kyoku: state.kyoku_idx,
            round_wind: state.round_wind,
            seat_wind: (seat + NUM_PLAYERS_3P as u8 - state.oya) % NUM_PLAYERS_3P as u8,
            ..Default::default()
        };

        let tedashi_log = tedashi_from_log(state);

        for pid in 0..NUM_PLAYERS_3P as u8 {
            let r = rel3(pid, seat);
            let player = &state.players[pid as usize];

            if pid == seat {
                for &t in &player.hand {
                    v.tehai[mortal_obs_deaka(t)] += 1;
                    match t {
                        16 => v.akas_in_hand[0] = true,
                        52 => v.akas_in_hand[1] = true,
                        88 => v.akas_in_hand[2] = true,
                        _ => {}
                    }
                }
            }

            // riichienv-3p leaves `discard_from_hand` empty, so prefer the
            // flags reconstructed from the mjai log; fall back to assuming no
            // tedashi when the log is unavailable.
            let tedashi = tedashi_log
                .as_ref()
                .and_then(|f| f.get(pid as usize).cloned())
                .unwrap_or_default();
            v.kawa[r] = build_kawa(
                &player.discards,
                &tedashi,
                &player.discard_is_riichi,
            );
            // NOTE: `kawa_overview` holds *raw* tile ids (0..135), because
            // `encode_tile_set` de-akas its input. The 4-player builder stores
            // de-aka'd ids, matching its own reference; the sanma reference was
            // captured from `libriichi3p`, which expects raw ids. The two forks
            // genuinely differ here, so this stays asymmetric on purpose.
            v.kawa_overview[r] = player.discards.clone();

            let (fuuro, ankan) = crate::mortal_obs::split_melds(&player.melds);
            v.fuuro_overview[r] = fuuro;
            v.ankan_overview[r] = ankan.into_iter().flatten().collect();

            v.scores[r] = player.score;
            v.riichi_declared[r] = player.riichi_declared || player.riichi_stage;
            // Replay distinguishes the server acceptance event from declaration.
            v.riichi_accepted[r] = state.mjai_log.iter().rev()
                .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                .take_while(|event| event["type"] != "start_kyoku")
                .any(|event| event["type"] == "reach_accepted" && event["actor"].as_u64() == Some(pid as u64));

            v.last_tedashis[r] = state.last_tedashis[pid as usize].map(|raw| KawaItem {
                tile: mortal_obs_deaka(raw),
                is_aka: mortal_obs_is_aka(raw),
                is_tedashi: true,
                ..Default::default()
            });
            v.riichi_sutehais[r] = state.riichi_sutehais[pid as usize].map(|raw| KawaItem {
                tile: mortal_obs_deaka(raw),
                is_aka: mortal_obs_is_aka(raw),
                is_riichi: true,
                is_tedashi: true,
                ..Default::default()
            });
        }

        // rank: how many seats are strictly ahead of us.
        let my_score = state.players[seat as usize].score;
        v.rank = state
            .players
            .iter()
            .filter(|p| p.score > my_score)
            .count()
            .min(2);

        v.dora_indicators = state.wall.dora_indicators.clone();
        v.doras_seen = v.dora_indicators.len().min(5) as u8;

        v.tiles_seen = tiles_seen_3p(state, seat, &v.dora_indicators);
        for &indicator in &v.dora_indicators {
            let dora = crate::tiles::next_dora_tile34(mortal_obs_deaka(indicator) as u8, 3) as usize;
            v.dora_factor[dora] = v.dora_factor[dora].saturating_add(1);
        }
        let mut red_seen = [false; 3];
        for &t in state.players[seat as usize].hand.iter()
            .chain(state.players.iter().flat_map(|p| p.discards.iter()))
            .chain(state.players.iter().flat_map(|p| p.melds.iter().flat_map(|m| m.tiles.iter())))
            .chain(v.dora_indicators.iter()) {
            match t { 16 => red_seen[0] = true, 52 => red_seen[1] = true, 88 => red_seen[2] = true, _ => {} }
        }
        let visible_reds = red_seen.iter().filter(|&&seen| seen).count();
        v.doras_seen = v.tiles_seen.iter().zip(v.dora_factor.iter())
            .map(|(&seen, &factor)| (seen as usize) * (factor as usize))
            .sum::<usize>().saturating_add(visible_reds).min(u8::MAX as usize) as u8;
        let absolute_owned = doras_owned_3p(state, seat, &v.dora_indicators);
        v.doras_owned = std::array::from_fn(|r| if r < NUM_PLAYERS_3P {
            absolute_owned[(seat as usize + r) % NUM_PLAYERS_3P]
        } else { 0 });

        // libriichi's `tiles_left` is the live wall, i.e. the tiles still
        // drawable excluding the 14-tile dead wall - exactly what
        // riichienv tracks as `drawable_count`.
        v.tiles_left = state.wall.drawable_count;

        let me = &state.players[seat as usize];
        v.at_furiten = !me.forbidden_discards.is_empty();
        // libriichi keeps `shanten` from the 3n+1 hand, so the drawn tile is
        // excluded even though `tehai` (the encoded feature) includes it.
        v.shanten = shanten_from_hand_vec(&me.hand, state.drawn_tile);

        v
    }

    /// Attach the legal-action-derived decision context for sanma.
    pub fn with_legal_actions_3p(mut self, state: &mut GameState3P, seat: u8) -> Self {
        use riichienv_core::action::ActionType;

        let obs = state.get_observation(seat);
        let legal = obs.legal_actions_method();
        let has = |t: ActionType| legal.iter().any(|a| a.action_type == t);

        self.can_discard = has(ActionType::Discard);
        self.can_riichi = has(ActionType::Riichi);
        self.can_nukidora = has(ActionType::Kita);
        self.can_pon = has(ActionType::Pon);
        self.can_daiminkan = has(ActionType::Daiminkan);
        self.can_ankan = has(ActionType::Ankan);
        self.can_kakan = has(ActionType::Kakan);
        self.can_agari = has(ActionType::Tsumo) || has(ActionType::Ron);
        // The replay state's is_first_turn can outlive our first discard.
        // Nine-terminals abort is only available on our first uninterrupted draw.
        self.can_ryukyoku = has(ActionType::KyushuKyuhai)
            && state.players[seat as usize].discards.is_empty()
            && state.players.iter().all(|p| p.melds.is_empty() && p.kita_tiles.is_empty());
        self.can_pass = legal.len() > 1 && legal.iter().any(|a| a.action_type == ActionType::Pass);

        // Sanma has no chi, so the chi flags stay false.

        self.discard_candidates = legal
            .iter()
            .filter(|a| a.action_type == ActionType::Discard)
            .filter_map(|a| a.tile)
            .collect();
        self.ankan_candidates = legal
            .iter()
            .filter(|a| a.action_type == ActionType::Ankan)
            .filter_map(|a| a.tile)
            .collect();
        self.kakan_candidates = legal
            .iter()
            .filter(|a| a.action_type == ActionType::Kakan)
            .filter_map(|a| a.tile)
            .collect();

        // libriichi retains the pre-draw hand's waits, including on a 14-tile turn.
        let mut pre_draw = self.tehai;
        if state.current_player == seat {
            if let Some(tile) = state.drawn_tile {
                let t = mortal_obs_deaka(tile);
                pre_draw[t] = pre_draw[t].saturating_sub(1);
            }
        }
        let span = (pre_draw.iter().map(|&n| n as usize).sum::<usize>() / 3) as u8;
        self.waits = [false; T];
        for t in 0..T {
            if (1..=7).contains(&t) || pre_draw[t] >= 4 { continue; }
            let mut probe = pre_draw;
            probe[t] += 1;
            self.waits[t] = riichienv_core::shanten::calc_shanten_from_counts_3p(&probe, span) < 0;
        }
        let me = &state.players[seat as usize];
        self.at_furiten = me.discards.iter().any(|&raw| self.waits[mortal_obs_deaka(raw)])
            || me.missed_agari_doujun || me.missed_agari_riichi;

        // -- discard quality flags -----------------------------------------
        // Verbatim from libriichi `PlayerState::update_shanten_discards`; see
        // the 4-player builder for the quoted match. "next" improves the hand,
        // "keep" holds it, a worsening discard is in neither set.
        if self.can_discard {
            let base = self.shanten;
            let mut seen: Vec<u8> = Vec::new();
            for &raw in &self.discard_candidates {
                let tid = mortal_obs_deaka(raw);
                if seen.contains(&(tid as u8)) || self.tehai[tid] == 0 {
                    continue;
                }
                seen.push(tid as u8);
                let mut after = self.tehai;
                after[tid] -= 1;
                let s_after = crate::state_features::shanten_same_span_3p(&after);
                if s_after < base as i8 {
                    self.next_shanten_discards.push(raw);
                } else if s_after == base as i8 {
                    self.keep_shanten_discards.push(raw);
                }
                if s_after == 0 && crate::state_features::has_any_wait_for(&after) {
                    self.unconditional_tenpai_discards.push(raw);
                }
            }
        }

        // -- kan-select flag -------------------------------------------------
        // Offering a kan is not the separate "which tile to kan" observation.
        self.at_kan_select = false;

        if let Some((_, tile)) = state.last_discard {
            self.last_kawa_tile = Some(KawaItem {
                tile: mortal_obs_deaka(tile),
                is_aka: mortal_obs_is_aka(tile),
                is_tedashi: true,
                ..Default::default()
            });
        }

        self
    }
}
