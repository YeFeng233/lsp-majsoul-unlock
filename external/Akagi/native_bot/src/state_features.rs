//! Self-relative seat view for Mortal observation encoding.
//!
//! Turns a [`GameState`] into the per-seat, self-relative feature set that
//! [`crate::mortal_obs::encode_mortal_obs`] consumes. Everything here is about
//! *rotation and derivation*; the channel layout lives in `mortal_obs.rs`.
//!
//! Seat order is `[self, shimocha, toimen, kamicha]` — index 0 is always the
//! deciding player, matching libriichi's `PlayerState` view.

use riichienv_core::state::GameState;
use riichienv_core::types::Meld;

use crate::mortal_obs::{build_kawa_with_dora, split_melds, KawaItem, SpCandidate, TILES};

/// Number of players this view supports.
pub const NUM_PLAYERS: usize = 4;

/// The self-relative view fed to the Mortal encoder.
#[derive(Debug, Clone)]
pub struct MortalSeatView {
    pub n_players: u8,

    /// Concealed hand as a 34-wide count vector.
    pub tehai: [u8; TILES],
    /// Which of 5m / 5p / 5s (red five) are held.
    pub akas_in_hand: [bool; 3],

    pub scores: [i32; NUM_PLAYERS],
    /// 0-based placing (0 = first).
    pub rank: usize,
    pub kyoku: u8,
    pub honba: u8,
    pub kyotaku: u32,
    pub round_wind: u8,
    pub seat_wind: u8,

    pub dora_indicators: Vec<u8>,
    pub doras_owned: [u8; NUM_PLAYERS],
    pub doras_seen: u8,

    /// Per-seat kawa, self-relative order, already de-aka'd into KawaItems.
    pub kawa: [Vec<Option<KawaItem>>; NUM_PLAYERS],
    /// Per-seat set of tiles visible in that seat's kawa (for kawa overview).
    pub kawa_overview: [Vec<u8>; NUM_PLAYERS],
    /// Per-seat open melds (chi/pon/daiminkan), for fuuro overview.
    pub fuuro_overview: [Vec<Meld>; NUM_PLAYERS],
    /// Per-seat concealed kans, for ankan overview.
    pub ankan_overview: [Vec<u8>; NUM_PLAYERS],

    pub tiles_seen: [u8; TILES],
    pub tiles_left: u8,

    pub last_tedashis: [Option<KawaItem>; NUM_PLAYERS],
    pub riichi_sutehais: [Option<KawaItem>; NUM_PLAYERS],
    pub riichi_declared: [bool; NUM_PLAYERS],
    pub riichi_accepted: [bool; NUM_PLAYERS],

    pub waits: [bool; TILES],
    pub at_furiten: bool,
    pub shanten: u8,

    // -- decision context -------------------------------------------------
    pub at_kan_select: bool,
    pub can_pass: bool,
    pub last_kawa_tile: Option<KawaItem>,
    pub dora_factor: [u8; TILES],

    pub can_discard: bool,
    pub discard_candidates: Vec<u8>,
    pub keep_shanten_discards: Vec<u8>,
    pub next_shanten_discards: Vec<u8>,
    pub unconditional_tenpai_discards: Vec<u8>,
    pub can_riichi: bool,
    pub can_nukidora: bool,
    pub can_chi_low: bool,
    pub can_chi_mid: bool,
    pub can_chi_high: bool,
    pub can_pon: bool,
    pub can_daiminkan: bool,
    pub can_ankan: bool,
    pub ankan_candidates: Vec<u8>,
    pub can_kakan: bool,
    pub kakan_candidates: Vec<u8>,
    pub can_agari: bool,
    pub can_ryukyoku: bool,

    // -- single-player table ----------------------------------------------
    pub max_ev: f32,
    pub sp_candidates: Vec<SpCandidate>,
    /// Native SP's best non-worsening discard, weighted by available copies.
    pub sp_best_required: Option<u8>,
}

impl Default for MortalSeatView {
    fn default() -> Self {
        Self {
            n_players: 4,
            tehai: [0; TILES],
            akas_in_hand: [false; 3],
            scores: [0; NUM_PLAYERS],
            rank: 0,
            kyoku: 0,
            honba: 0,
            kyotaku: 0,
            round_wind: 0,
            seat_wind: 0,
            dora_indicators: Vec::new(),
            doras_owned: [0; NUM_PLAYERS],
            doras_seen: 0,
            kawa: Default::default(),
            kawa_overview: Default::default(),
            fuuro_overview: Default::default(),
            ankan_overview: Default::default(),
            tiles_seen: [0; TILES],
            tiles_left: 69,
            last_tedashis: [None; NUM_PLAYERS],
            riichi_sutehais: [None; NUM_PLAYERS],
            riichi_declared: [false; NUM_PLAYERS],
            riichi_accepted: [false; NUM_PLAYERS],
            waits: [false; TILES],
            at_furiten: false,
            shanten: 6,
            at_kan_select: false,
            can_pass: false,
            last_kawa_tile: None,
            dora_factor: [0; TILES],
            can_discard: true,
            discard_candidates: Vec::new(),
            keep_shanten_discards: Vec::new(),
            next_shanten_discards: Vec::new(),
            unconditional_tenpai_discards: Vec::new(),
            can_riichi: false,
            can_nukidora: false,
            can_chi_low: false,
            can_chi_mid: false,
            can_chi_high: false,
            can_pon: false,
            can_daiminkan: false,
            can_ankan: false,
            ankan_candidates: Vec::new(),
            can_kakan: false,
            kakan_candidates: Vec::new(),
            can_agari: false,
            can_ryukyoku: false,
            max_ev: 0.0,
            sp_best_required: None,
            sp_candidates: Vec::new(),
        }
    }
}

/// Map an absolute player id to its index in self-relative order.
fn rel(pid: u8, seat: u8) -> usize {
    ((pid + NUM_PLAYERS as u8 - seat) as usize) % NUM_PLAYERS
}

fn dora_type(raw: u8) -> usize {
    let t = (raw / 4) as usize;
    match t {
        0..=8 => (t + 1) % 9,
        9..=17 => 9 + (t - 9 + 1) % 9,
        18..=26 => 18 + (t - 18 + 1) % 9,
        27..=30 => 27 + (t - 27 + 1) % 4,
        31..=33 => 31 + (t - 31 + 1) % 3,
        _ => t.min(TILES - 1),
    }
}

fn dora_flags(indicators: &[u8]) -> [bool; TILES] {
    let mut flags = [false; TILES];
    for &ind in indicators {
        flags[dora_type(ind)] = true;
    }
    flags
}

fn count_doras(raw_tiles: &[u8], dora_types: &[bool; TILES]) -> u8 {
    raw_tiles
        .iter()
        .filter(|&&t| dora_types[crate::mortal_obs::mortal_obs_deaka(t)])
        .count()
        .min(12) as u8
}

/// Shanten for a concealed-hand count vector.
///
/// `tehai_len_div3` is the *concealed hand length divided by three*, not the
/// meld count: `calc_normal` uses it as a table index and
/// `calc_shanten_from_counts` only checks chiitoitsu/kokushi when it is `>= 4`.
/// The two differ by a constant (`len / 3 == 4 - melds` for a full hand), and
/// feeding the meld count instead makes a closed hand look like an open one —
/// a scattered 13-tile hand comes back as tenpai.
fn calc_shanten(tehai: &[u8; TILES]) -> u8 {
    let total: usize = tehai.iter().map(|&c| c as usize).sum();
    let shanten =
        riichienv_core::shanten::calc_shanten_from_counts(tehai, (total / 3) as u8);
    shanten.clamp(0, 6) as u8
}

/// Shanten with `tehai_len_div3` held at the *pre-discard* value.
///
/// libriichi's `update_shanten_discards` calls `shanten::calc_all(&tehai,
/// self.tehai_len_div3)` inside the loop without recomputing the length, so the
/// table index stays at `4` (for a full hand) even though the vector it is
/// looking at has one fewer tile. Deriving the length from the shrunken vector
/// instead gives `3`, which selects a different table entry.
fn calc_shanten_same_span(tehai: &[u8; TILES]) -> i8 {
    riichienv_core::shanten::calc_shanten_from_counts(tehai, 4)
}

/// Shanten for a sanma hand, clamped to the encoded range.
///
/// Exposed for `state_features_3p` and `sp_tables` so the 3-player path uses
/// `calc_shanten_from_counts_3p` (which relocates the manzu tiles that sanma
/// does not use) instead of the 4-player routine.
pub fn shanten_for(tehai: &[u8; TILES]) -> u8 {
    let total: usize = tehai.iter().map(|&c| c as usize).sum();
    let s =
        riichienv_core::shanten::calc_shanten_from_counts_3p(tehai, (total / 3) as u8);
    s.clamp(0, 6) as u8
}

/// Sanma counterpart of [`calc_shanten_same_span`]: `tehai_len_div3` stays at the
/// pre-discard value while the vector has one fewer tile.
pub fn shanten_same_span_3p(tehai: &[u8; TILES]) -> i8 {
    riichienv_core::shanten::calc_shanten_from_counts_3p(tehai, 4)
}

/// [`has_any_wait`] using the sanma shanten routine.
pub fn has_any_wait_for(tehai: &[u8; TILES]) -> bool {    for t in 0..TILES {
        if tehai[t] >= 4 {
            continue;
        }
        let mut probe = *tehai;
        probe[t] += 1;
        if shanten_for(&probe) == 0 {
            return true;
        }
    }
    false
}

/// True when a tenpai hand (shanten 0) has at least one winning tile left.
///
/// Used for the `unconditional_tenpai_discards` plane: a discard that leaves a
/// hand which wins on *some* draw, independent of which tile comes next.
fn has_any_wait(tehai: &[u8; TILES]) -> bool {
    for t in 0..TILES {
        if tehai[t] >= 4 {
            continue;
        }
        let mut probe = *tehai;
        probe[t] += 1;
        if calc_shanten(&probe) == 0 {
            return true;
        }
    }
    false
}

impl MortalSeatView {
    ///
    /// This fills the structural fields (hand, melds, kawa, tiles_seen, winds,
    /// scores). The *decision* fields (legal-action flags, shanten, waits and
    /// the single-player table) depend on the legal-action set, which the
    /// caller owns — see [`Self::with_legal_actions`].
    pub fn from_state(state: &GameState, seat: u8) -> Self {
        let mut v = Self {
            n_players: NUM_PLAYERS as u8,
            honba: state.honba,
            kyotaku: state.riichi_sticks,
            kyoku: state.kyoku_idx,
            round_wind: state.round_wind,
            seat_wind: (seat + NUM_PLAYERS as u8 - state.oya) % NUM_PLAYERS as u8,
            ..Default::default()
        };

        let dora_types = dora_flags(&state.wall.dora_indicators);
        let me = &state.players[seat as usize];

        // -- hands, melds, kawa, per-seat features ------------------------
        for pid in 0..NUM_PLAYERS as u8 {
            let r = rel(pid, seat);
            let player = &state.players[pid as usize];

            if pid == seat {
                for &t in &player.hand {
                    v.tehai[crate::mortal_obs::mortal_obs_deaka(t)] += 1;
                    match t {
                        16 => v.akas_in_hand[0] = true,
                        52 => v.akas_in_hand[1] = true,
                        88 => v.akas_in_hand[2] = true,
                        _ => {}
                    }
                }
            }

            let kawa = build_kawa_with_dora(
                &player.discards,
                &player.discard_from_hand,
                &player.discard_is_riichi,
                &player
                    .discards
                    .iter()
                    .map(|&t| dora_types[crate::mortal_obs::mortal_obs_deaka(t)])
                    .collect::<Vec<_>>(),
            );
            v.kawa[r] = kawa.clone();
            v.kawa_overview[r] = player
                .discards
                .iter()
                .map(|&t| crate::mortal_obs::mortal_obs_deaka(t) as u8)
                .collect();

            let (fuuro, ankan) = split_melds(&player.melds);
            v.fuuro_overview[r] = fuuro;
            v.ankan_overview[r] = ankan.into_iter().flatten().collect();

            v.scores[r] = player.score;
            v.riichi_declared[r] = player.riichi_declared;

            v.last_tedashis[r] = state.last_tedashis[pid as usize].map(|raw| KawaItem {
                tile: crate::mortal_obs::mortal_obs_deaka(raw),
                is_aka: crate::mortal_obs::mortal_obs_is_aka(raw),
                is_tedashi: true,
                ..Default::default()
            });
            v.riichi_sutehais[r] = state.riichi_sutehais[pid as usize].map(|raw| KawaItem {
                tile: crate::mortal_obs::mortal_obs_deaka(raw),
                is_aka: crate::mortal_obs::mortal_obs_is_aka(raw),
                is_riichi: true,
                is_tedashi: true,
                ..Default::default()
            });
        }

        // -- rank ---------------------------------------------------------
        let my_score = state.players[seat as usize].score;
        v.rank = state
            .players
            .iter()
            .filter(|p| p.score > my_score)
            .count()
            .min(3);

        // -- dora ---------------------------------------------------------
        v.dora_indicators = state.wall.dora_indicators.clone();
        v.doras_seen = v.dora_indicators.len().min(5) as u8;
        for (_i, player) in state.players.iter().enumerate() {
            for m in &player.melds {
                if m.meld_type == riichienv_core::types::MeldType::Kakan {
                    // A kakan promotes one indicator; count it as seen.
                }
            }
        }

        // -- tiles seen / left --------------------------------------------
        // `tiles_seen` is every copy of a tile the seat can account for: all
        // four ponds, all open and concealed melds, the dora indicators, and
        // the seat's own hand. libriichi feeds this straight into its wait and
        // ukeire tables, so omitting our own hand would understate how dead a
        // wait is.
        for player in state.players.iter() {
            for &t in player.discards.iter() {
                let tid = crate::mortal_obs::mortal_obs_deaka(t);
                v.tiles_seen[tid] = v.tiles_seen[tid].saturating_add(1);
            }
            for m in &player.melds {
                for &t in &m.tiles {
                    let tid = crate::mortal_obs::mortal_obs_deaka(t);
                    v.tiles_seen[tid] = v.tiles_seen[tid].saturating_add(1);
                }
            }
        }
        for &t in &v.dora_indicators {
            let tid = crate::mortal_obs::mortal_obs_deaka(t);
            v.tiles_seen[tid] = v.tiles_seen[tid].saturating_add(1);
        }
        for &t in &me.hand {
            let tid = crate::mortal_obs::mortal_obs_deaka(t);
            v.tiles_seen[tid] = v.tiles_seen[tid].saturating_add(1);
        }

        // `tiles_left` is the live wall count; libriichi divides by 69.
        v.tiles_left = state.wall.tiles.len().min(69) as u8;

        // -- dora ----------------------------------------------------------
        // `doras_owned` counts dora among each seat's *visible* tiles (melds and
        // pond), plus our own hand for our seat. `doras_seen` counts the
        // indicators themselves, which is what libriichi's `doras_unseen` plane
        // needs: `indicators * 4 + 3 - doras_seen`.
        for pid in 0..NUM_PLAYERS as u8 {
            let r = rel(pid, seat);
            let player = &state.players[pid as usize];
            let mut owned: u8 = 0;
            owned = owned.saturating_add(
                player
                    .melds
                    .iter()
                    .flat_map(|m| m.tiles.iter().copied())
                    .filter(|&t| dora_types[crate::mortal_obs::mortal_obs_deaka(t)])
                    .count() as u8,
            );
            if pid == seat {
                owned = owned.saturating_add(count_doras(&player.hand, &dora_types));
            }
            v.doras_owned[r] = owned.min(12);
        }
        v.doras_seen = state.wall.dora_indicators.len().min(5) as u8;
        // Per-tile dora weight, used for the call-context and last-tedashi
        // planes (`state.dora_factor` upstream).
        for (t, slot) in v.dora_factor.iter_mut().enumerate() {
            let mut n = 0u8;
            for ind in &state.wall.dora_indicators {
                if dora_type(*ind) == t {
                    n = n.saturating_add(1);
                }
            }
            *slot = n;
        }

        // -- furiten -------------------------------------------------------
        // A seat is furiten when one of its own waits appears in its own pond
        // (permanent) or when it passed on a winning tile this cycle (temporary
        // — `missed_agari_doujun`). `forbidden_discards` is kuikae, not furiten.
        let own_discard_types: [bool; TILES] = {
            let mut f = [false; TILES];
            for &t in &me.discards {
                f[crate::mortal_obs::mortal_obs_deaka(t)] = true;
            }
            f
        };
        let mut hand = v.tehai;
        if hand.iter().map(|&n| n as usize).sum::<usize>() % 3 == 2 {
            if let Some(t) = state.drawn_tile { hand[crate::mortal_obs::mortal_obs_deaka(t)] -= 1; }
        }
        let span = (hand.iter().map(|&n| n as usize).sum::<usize>() / 3) as u8;
        for t in 0..TILES {
            if hand[t] < 4 {
                hand[t] += 1;
                v.waits[t] = riichienv_core::shanten::calc_shanten_from_counts(&hand, span) == -1;
                hand[t] -= 1;
            }
        }
        let wait_hits_own_pond = v
            .waits
            .iter()
            .enumerate()
            .any(|(t, &w)| w && own_discard_types[t]);
        v.at_furiten = wait_hits_own_pond || me.missed_agari_doujun || me.missed_agari_riichi;

        // -- shanten -------------------------------------------------------
        // The encoder one-hots `shanten` capped at 6, and the SP-table planes
        // are only meaningful at low shanten.
        v.shanten = calc_shanten(&v.tehai);

        v
    }

    /// Attach the legal-action-derived decision context.
    ///
    /// `legal` is the action list the engine is choosing over; the flags here
    /// mirror what libriichi's `Cans` computes from the same set.
    ///
    /// Takes `&mut GameState` because `get_observation` needs a mutable borrow
    /// to snapshot the legal-action set.
    pub fn with_legal_actions(mut self, state: &mut GameState, seat: u8) -> Self {
        use riichienv_core::action::ActionType;

        let obs = state.get_observation(seat);
        let legal = obs.legal_actions_method();

        let has = |t: ActionType| legal.iter().any(|a| a.action_type == t);

        self.can_discard = has(ActionType::Discard);
        self.can_riichi = has(ActionType::Riichi);
        self.can_pon = has(ActionType::Pon);
        self.can_daiminkan = has(ActionType::Daiminkan);
        self.can_ankan = has(ActionType::Ankan);
        self.can_kakan = has(ActionType::Kakan);
        self.can_agari = has(ActionType::Tsumo) || has(ActionType::Ron);
        self.can_ryukyoku = has(ActionType::KyushuKyuhai);
        self.can_pass = legal.len() > 1
            && legal.iter().any(|a| a.action_type == ActionType::Pass);

        // Chi variants are distinguished by the called tile's position.
        for a in legal.iter() {
            if a.action_type == ActionType::Chi {
                if let Some(t) = a.tile {
                    let called = crate::mortal_obs::mortal_obs_deaka(t);
                    let mut consumed: Vec<usize> = a
                        .consume_tiles
                        .iter()
                        .map(|&x| crate::mortal_obs::mortal_obs_deaka(x))
                        .collect();
                    consumed.sort_unstable();
                    if consumed.len() >= 2 {
                        if called < consumed[0] {
                            self.can_chi_low = true;
                        } else if called > consumed[1] {
                            self.can_chi_high = true;
                        } else {
                            self.can_chi_mid = true;
                        }
                    }
                }
            }
        }

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

        self.waits = [false; TILES];
        for &w in &obs.waits {
            self.waits[(w as usize).min(TILES - 1)] = true;
        }

        // -- discard quality flags -----------------------------------------
        // Verbatim from libriichi `PlayerState::update_shanten_discards`:
        //
        //     match shanten_after.cmp(&self.shanten) {
        //         Ordering::Less    => next_shanten_discards[tid] = true,
        //         Ordering::Equal   => keep_shanten_discards[tid] = true,
        //         _                 => (),
        //     }
        //
        // So "next" means the discard *improves* the hand by one step, "keep"
        // means it holds the current shanten, and a discard that makes things
        // worse is recorded in neither. The two are not a partition of the
        // candidates: a worsening discard belongs to no set.
        //
        // `tehai_len_div3` is constant across the loop upstream (it describes
        // the hand's 3n+2 length, not the length after the discard), so it is
        // passed unchanged here too.
        if self.can_discard {
            let base = self.shanten as i8;
            let mut seen: Vec<u8> = Vec::new();
            for &raw in &self.discard_candidates {
                let tid = crate::mortal_obs::mortal_obs_deaka(raw);
                if seen.contains(&(tid as u8)) {
                    continue;
                }
                seen.push(tid as u8);
                if self.tehai[tid] == 0 {
                    continue;
                }
                let mut after = self.tehai;
                after[tid] -= 1;
                let s_after = calc_shanten_same_span(&after);
                if s_after < base {
                    self.next_shanten_discards.push(raw);
                } else if s_after == base {
                    self.keep_shanten_discards.push(raw);
                }
                if s_after == 0 && has_any_wait(&after) {
                    self.unconditional_tenpai_discards.push(raw);
                }
            }
        }

        // -- kan-select flag -------------------------------------------------
        // libriichi sets this while the player is choosing which kan to declare.
        self.at_kan_select = false; // Only set for a separate kan-selection observation, not its offer window.

        // -- single-player table ---------------------------------------------
        // The last 123 channels are EV, required tiles and the per-turn
        // tenpai/win/EV planes. Without them the model sees a block of zeros,
        // which it reads as "no line has any value". See `crate::sp_tables`.
        if self.can_discard {
            let discard_raw: Vec<u8> = self
                .discard_candidates
                .iter()
                .copied()
                .collect();
            self.sp_candidates = crate::sp_tables::build_sp_candidates(
                &self.tehai,
                &discard_raw,
                self.tiles_left,
                &self.tiles_seen,
            );
            self.max_ev = crate::sp_tables::max_ev(&self.sp_candidates);
        }

        if let Some((_, tile)) = state.last_discard {
            self.last_kawa_tile = Some(KawaItem {
                tile: crate::mortal_obs::mortal_obs_deaka(tile),
                is_aka: crate::mortal_obs::mortal_obs_is_aka(tile),
                is_tedashi: true,
                ..Default::default()
            });
        }

        self
    }
}
