//! Native SP tables for Akagi's 775-channel sanma contract.
//! The reference's look-ahead uses four-seat turn spacing even for sanma;
//! reproduce the trained feature protocol rather than substituting new odds.
use crate::{mortal_obs::SpCandidate, state_features::MortalSeatView};
use mortal_sp::{
    algo::{
        agari::AgariCalculator,
        shanten,
        sp::{CandidateColumn, InitState, SPCalculator},
    },
    tile::Tile,
};
use riichienv_core::{state_3p::GameState3P, types::MeldType};
use std::{
    cell::RefCell,
    hash::{DefaultHasher, Hash, Hasher},
};

#[derive(Clone)]
struct Tables {
    max_ev: f32,
    best: Option<u8>,
    candidates: Vec<SpCandidate>,
}

thread_local! { static CACHE: RefCell<Option<(u64, Tables)>> = const { RefCell::new(None) }; }

fn raw(tile: Tile) -> u8 {
    match tile.as_u8() {
        34 => 16,
        35 => 52,
        36 => 88,
        37 => 0,
        t => t * 4 + u8::from(matches!(t, 4 | 13 | 22)),
    }
}

fn tile(raw: u8) -> Tile {
    Tile::try_from(match raw {
        16 => 34,
        52 => 35,
        88 => 36,
        t => t / 4,
    } as usize)
    .unwrap()
}

fn winning_hans(view: &MortalSeatView, state: &GameState3P, seat: u8, menzen: bool) -> u8 {
    let mut first = true;
    let mut double = false;
    let mut ippatsu = false;
    let mut rinshan = false;
    let mut chankan = false;
    for line in &state.mjai_log {
        let Ok(e) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let own = e["actor"].as_u64() == Some(seat as u64);
        match e["type"].as_str().unwrap_or("") {
            "reach" if own => double = first,
            "reach_accepted" if own => ippatsu = true,
            "dahai" if own => {
                first = false;
                ippatsu = false;
                rinshan = false;
            }
            "pon" | "daiminkan" | "ankan" | "kakan" | "kita" | "nukidora" => {
                first = false;
                ippatsu = false;
                rinshan = own;
                chankan = !own && matches!(e["type"].as_str(), Some("kakan" | "kita" | "nukidora"));
            }
            "tsumo" | "dahai" => chankan = false,
            _ => {}
        }
    }
    u8::from(view.riichi_accepted[0])
        + u8::from(double)
        + u8::from(ippatsu)
        + if view.can_discard {
            u8::from(menzen) + u8::from(rinshan) + u8::from(view.tiles_left == 0 && !rinshan)
        } else {
            u8::from(view.tiles_left == 0) + u8::from(chankan)
        }
}

/// A tenpai discard must leave at least one live, yaku-bearing ron wait and
/// none of its waits may already be in our river (including the new discard).
pub fn unconditional_tenpai(
    view: &MortalSeatView,
    state: &GameState3P,
    seat: u8,
    hand: &[u8; 34],
    discard: usize,
) -> bool {
    if view.tiles_left == 0 {
        return false;
    }
    if let Some(t) = state.drawn_tile {
        if view.waits[(t / 4) as usize] {
            return false;
        }
        if view.riichi_accepted[0] {
            return !view.at_furiten && (t / 4) as usize == discard;
        }
    } else if shanten::calc_all(
        &view.tehai,
        (view.tehai.iter().map(|&n| n as usize).sum::<usize>() / 3) as u8,
    ) < 0
    {
        return false;
    }
    let me = &state.players[seat as usize];
    let mut pons = Vec::new();
    let mut minkans = Vec::new();
    let mut ankans = Vec::new();
    for meld in &me.melds {
        let Some(&t) = meld.tiles.first() else {
            return false;
        };
        match meld.meld_type {
            MeldType::Pon => pons.push(t / 4),
            MeldType::Ankan => ankans.push(t / 4),
            MeldType::Kakan | MeldType::Daiminkan => minkans.push(t / 4),
            _ => return false,
        }
    }
    let span = (hand.iter().map(|&n| n as usize).sum::<usize>() / 3) as u8;
    let mut live_yaku = false;
    for t in 0..34 {
        if t == discard || (1..=7).contains(&t) || hand[t] >= 4 {
            continue;
        }
        let mut complete = *hand;
        complete[t] += 1;
        if shanten::calc_all(&complete, span) >= 0 {
            continue;
        }
        if me.discards.iter().any(|&d| d / 4 == t as u8) {
            return false;
        }
        if view.tiles_seen[t] >= 4 || live_yaku {
            continue;
        }
        live_yaku = AgariCalculator {
            tehai: &complete,
            is_menzen: me.melds.iter().all(|m| !m.opened),
            chis: &[],
            pons: &pons,
            minkans: &minkans,
            ankans: &ankans,
            bakaze: 27 + view.round_wind,
            jikaze: 27 + view.seat_wind,
            winning_tile: t as u8,
            is_ron: true,
        }
        .has_yaku();
    }
    live_yaku
}

pub fn populate(view: &mut MortalSeatView, state: &GameState3P, seat: u8) {
    let me = &state.players[seat as usize];
    let mut pons = Vec::new();
    let mut minkans = Vec::new();
    let mut ankans = Vec::new();
    for meld in &me.melds {
        let Some(&first) = meld.tiles.first() else {
            return;
        };
        match meld.meld_type {
            MeldType::Pon => pons.push(first / 4),
            MeldType::Daiminkan | MeldType::Kakan => minkans.push(first / 4),
            MeldType::Ankan => ankans.push(first / 4),
            _ => return,
        }
    }
    let is_menzen = me.melds.iter().all(|m| !m.opened);
    let indicators: Vec<_> = view.dora_indicators.iter().copied().map(tile).collect();
    let mut red_seen = [false; 3];
    for &t in me
        .hand
        .iter()
        .chain(state.players.iter().flat_map(|p| p.discards.iter()))
        .chain(
            state
                .players
                .iter()
                .flat_map(|p| p.melds.iter().flat_map(|m| m.tiles.iter())),
        )
        .chain(view.dora_indicators.iter())
    {
        match t {
            16 => red_seen[0] = true,
            52 => red_seen[1] = true,
            88 => red_seen[2] = true,
            _ => {}
        }
    }
    let fuuro_doras = me
        .melds
        .iter()
        .flat_map(|m| m.tiles.iter())
        .map(|&t| view.dora_factor[(t / 4) as usize] + u8::from(matches!(t, 16 | 52 | 88)))
        .sum::<u8>();
    let double_riichi = view.can_discard
        && me.discards.is_empty()
        && state
            .players
            .iter()
            .all(|p| p.melds.is_empty() && p.kita_tiles.is_empty());
    let win_hans = winning_hans(view, state, seat, is_menzen);
    let mut hash = DefaultHasher::new();
    (
        view.tehai,
        view.akas_in_hand,
        view.tiles_seen,
        red_seen,
        &indicators,
        &pons,
        &minkans,
        &ankans,
        view.round_wind,
        view.seat_wind,
    )
        .hash(&mut hash);
    (
        view.tiles_left,
        view.can_discard,
        view.riichi_accepted[0],
        state.drawn_tile,
        state.last_discard,
        fuuro_doras,
        double_riichi,
        view.scores[0] >= 1000,
        view.can_agari,
        win_hans,
        me.kita_tiles.len(),
    )
        .hash(&mut hash);
    let key = hash.finish();
    let tables = CACHE.with(|cache| {
        if let Some((old, tables)) = cache.borrow().as_ref() {
            if *old == key {
                return tables.clone();
            }
        }
        let mut hand = view.tehai;
        let mut akas = view.akas_in_hand;
        let mut can_discard = view.can_discard;
        let span = (hand.iter().map(|&n| n as usize).sum::<usize>() / 3) as u8;
        let cur_shanten = shanten::calc_all(&hand, span);
        let target = state
            .last_discard
            .map(|(pid, _)| (pid + 3 - seat) % 3)
            .unwrap_or(0);
        let remaining = if can_discard {
            view.tiles_left
        } else {
            view.tiles_left.saturating_sub(4 - target)
        };
        let turns = remaining / 4;
        let after_riichi = can_discard && view.riichi_accepted[0];
        if after_riichi {
            if let Some(drawn) = state.drawn_tile {
                hand[(drawn / 4) as usize] = hand[(drawn / 4) as usize].saturating_sub(1);
                match drawn {
                    16 => akas[0] = false,
                    52 => akas[1] = false,
                    88 => akas[2] = false,
                    _ => {}
                }
                can_discard = false;
            }
        }
        let calculate = || -> anyhow::Result<Tables> {
            anyhow::ensure!(cur_shanten >= 0 && (1..=17).contains(&turns));
            anyhow::ensure!(view.tiles_seen.iter().all(|&n| n <= 4));
            let calculator = SPCalculator {
                tehai_len_div3: span,
                is_menzen,
                chis: &[],
                pons: &pons,
                minkans: &minkans,
                ankans: &ankans,
                bakaze: 27 + view.round_wind,
                jikaze: 27 + view.seat_wind,
                num_doras_in_fuuro: fuuro_doras,
                prefer_riichi: view.scores[0] >= 1000,
                dora_indicators: &indicators,
                calc_double_riichi: double_riichi,
                calc_haitei: remaining % 4 == 0,
                sort_result: true,
                maximize_win_prob: false,
                calc_tegawari: false,
                calc_shanten_down: false,
            };
            let mut candidates = calculator.calc(
                InitState {
                    tehai: hand,
                    akas_in_hand: akas,
                    tiles_seen: view.tiles_seen,
                    akas_seen: red_seen,
                },
                can_discard,
                turns,
                cur_shanten,
            )?;
            if after_riichi {
                if let (Some(first), Some(drawn)) = (candidates.first_mut(), state.drawn_tile) {
                    first.tile = tile(drawn);
                }
            }
            let max_ev = candidates
                .first()
                .and_then(|c| c.exp_values.first())
                .copied()
                .unwrap_or(0.0);
            let best = candidates
                .iter()
                .max_by(|a, b| a.cmp(b, CandidateColumn::NotShantenDown))
                .map(|c| raw(c.tile));
            Ok(Tables {
                max_ev,
                best,
                candidates: candidates
                    .into_iter()
                    .map(|c| SpCandidate {
                        tile: raw(c.tile),
                        shanten_down: c.shanten_down,
                        required_tiles: c.required_tiles.iter().map(|r| raw(r.tile)).collect(),
                        tenpai_probs: c.tenpai_probs.into_iter().collect(),
                        win_probs: c.win_probs.into_iter().collect(),
                        exp_values: c.exp_values.into_iter().collect(),
                    })
                    .collect(),
            })
        };
        let tables = calculate().unwrap_or_else(|_| {
            if !view.can_agari {
                return Tables {
                    max_ev: 0.0,
                    best: None,
                    candidates: Vec::new(),
                };
            }
            let winning = state
                .drawn_tile
                .or_else(|| state.last_discard.map(|(_, t)| t));
            let mut complete = view.tehai;
            if !view.can_discard {
                if let Some(t) = winning {
                    complete[(t / 4) as usize] += 1;
                }
            }
            let max_ev = if view.can_agari && double_riichi {
                mortal_sp::algo::point::Point::yakuman(view.seat_wind == 0, 1)
                    .tsumo_total(view.seat_wind == 0) as f32
            } else {
                winning
                    .and_then(|t| {
                        AgariCalculator {
                            tehai: &complete,
                            is_menzen,
                            chis: &[],
                            pons: &pons,
                            minkans: &minkans,
                            ankans: &ankans,
                            bakaze: 27 + view.round_wind,
                            jikaze: 27 + view.seat_wind,
                            winning_tile: t / 4,
                            is_ron: !view.can_discard,
                        }
                        .agari(
                            win_hans,
                            view.doras_owned[0]
                                + me.kita_tiles.len() as u8
                                + if view.can_discard {
                                    0
                                } else {
                                    view.dora_factor[(t / 4) as usize]
                                        + u8::from(matches!(t, 16 | 52 | 88))
                                },
                        )
                    })
                    .map(|a| {
                        a.point(view.seat_wind == 0)
                            .tsumo_total(view.seat_wind == 0) as f32
                    })
                    .unwrap_or(0.0)
            };
            Tables {
                max_ev,
                best: None,
                candidates: Vec::new(),
            }
        });
        *cache.borrow_mut() = Some((key, tables.clone()));
        tables
    });
    view.max_ev = tables.max_ev;
    view.sp_best_required = tables.best;
    view.sp_candidates = tables.candidates;
}
