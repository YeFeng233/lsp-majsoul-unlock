//! Mortal observation encoding from a live [`GameState`].
//!
//! This is a partial port of `libriichi/src/state/obs_repr.rs` from
//! Equim-chan/Mortal and Akagi's 775-channel sanma contract. It has not passed
//! complete upstream observation parity; see docs/mortal-sanma-quality.md.
//!
//! Why this module exists alongside [`crate::obs::encode`]: that encoder feeds
//! Akagi's *own* compact feature format (`channels(num_players)` planes), which
//! is a different layout from Mortal's 1012. Mortal's weights only understand
//! the layout below, and a model fed the wrong layout returns plausible logits
//! from wrong inputs — no error, just bad play. So the two encoders are
//! deliberately separate.
//!
//! Output is `vec![f32; C * 34]` with `buf[ch * 34 + tile]`, exactly matching
//! `obs_shape(4) = (1012, 34)`.

use riichienv_core::state::GameState;
use riichienv_core::types::{Meld, MeldType};

use crate::state_features::MortalSeatView;

/// Mortal's tile axis width.
pub const TILES: usize = 34;
/// `obs_shape(4).0` for the 4-player build.
pub const CHANNELS_V4: usize = 1012;
/// `obs_shape(4).0` for the 3-player build (`libriichi3p`'s own version 4).
pub const CHANNELS_SANMA: usize = 775;
/// `MAX_NUM_TURNS` in libriichi, used for the SP-table planes.
pub const MAX_NUM_TURNS: usize = 17;

/// Feature-geometry differences between the 4-player build and `libriichi3p`.
///
/// Everything else about the two layouts is identical, so the encoder body is
/// shared and these are the only knobs. The 3-player values were read off
/// ground-truth buffers produced by the real `libriichi3p` binary - see
/// `tools/sanma/README.md`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub channels: usize,
    pub num_players: usize,
    /// `rank` one-hot width. 4p: 4. 3p: 3.
    pub rank_channels: usize,
    /// `kyoku` one-hot width. 4p: 4. 3p: 3.
    pub kyoku_channels: usize,
    /// Score-plane divisors, applied in order.
    pub score_div: [f32; 2],
    /// `doras_owned` planes. Both builds emit 4, even in 3p.
    pub doras_owned_channels: usize,
    /// `riichi_declared[1..]` width. 4p: 3. 3p: 2 (one per other seat).
    pub riichi_declared_channels: usize,
    /// `riichi_accepted[1..]` width. 4p: 3. 3p: 2.
    pub riichi_accepted_channels: usize,
    /// Chi (4p) or nukidora (3p) block width. 4p: 3. 3p: 1.
    pub call_block_channels: usize,
    /// `tiles_left` divisor; both builds use 69.
    pub tiles_left_div: f32,
    /// Divisor for `doras_unseen`; both use 23.
    pub doras_unseen_div: f32,
}

impl Layout {
    /// The 4-player channel layout; matching geometry does not establish value parity.
    pub const V4: Layout = Layout {
        channels: CHANNELS_V4,
        num_players: 4,
        rank_channels: 4,
        kyoku_channels: 4,
        score_div: [100_000.0, 30_000.0],
        doras_owned_channels: 4,
        riichi_declared_channels: 3,
        riichi_accepted_channels: 3,
        call_block_channels: 3,
        tiles_left_div: 69.0,
        doras_unseen_div: 23.0,
    };

    /// The 3-player layout as emitted by `libriichi3p` (`obs_shape(4) = 775`).
    pub const SANMA: Layout = Layout {
        channels: CHANNELS_SANMA,
        num_players: 3,
        rank_channels: 3,
        kyoku_channels: 3,
        // Measured: a 25000 score plane reads 0.2381 and 0.625, i.e.
        // 25000/105000 and 25000/40000.
        score_div: [105_000.0, 40_000.0],
        doras_owned_channels: 4,
        riichi_declared_channels: 2,
        riichi_accepted_channels: 2,
        call_block_channels: 1,
        tiles_left_div: 69.0,
        doras_unseen_div: 23.0,
    };
}

const SELF_KAWA_ITEM_CHANNELS: usize = 4;
const KAWA_ITEM_CHANNELS: usize = 8;
/// `encode_tile_set`: 4 count rows + 3 aka rows.
const TILE_SET_CHANNELS: usize = 7;

const HONBA_CAP: usize = 10;
const SHANTEN_CAP: usize = 6;

/// Accumulates the channel-major `[C, 34]` buffer with an explicit cursor, the
/// same way `Simple2DArray<34, f32>` + `self.idx` do upstream. The cursor
/// discipline is what makes the layout checkable.
struct Plane {
    buf: Vec<f32>,
    idx: usize,
}

impl Plane {
    /// Number of channels this plane holds.
    fn channels(&self) -> usize {
        self.buf.len() / TILES
    }
}

impl Plane {
    fn new(channels: usize) -> Self {
        Self {
            buf: vec![0.0; channels * TILES],
            idx: 0,
        }
    }

    #[inline]
    fn assign(&mut self, ch: usize, tile: usize, v: f32) {
        if ch < self.channels() && tile < TILES {
            self.buf[ch * TILES + tile] = v;
        }
    }

    /// `fill(row, v)`: set every tile column in one channel.
    #[inline]
    fn fill(&mut self, ch: usize, v: f32) {
        if ch < self.channels() {
            let base = ch * TILES;
            for i in 0..TILES {
                self.buf[base + i] = v;
            }
        }
    }

    /// `assign_rows(idx, tile, n, v)`: column `tile` across n consecutive rows.
    #[inline]
    fn assign_rows(&mut self, base: usize, tile: usize, n: usize, v: f32) {
        for ch in base..(base + n).min(self.channels()) {
            self.assign(ch, tile, v);
        }
    }

    #[inline]
    fn advance(&mut self, n: usize) {
        self.idx += n;
    }
}

/// libriichi's `encode_tile_set`: four count rows followed by three aka rows.
fn encode_tile_set(p: &mut Plane, tiles: &[u8]) {
    let mut counts = [0usize; TILES];
    for &raw in tiles {
        let tid = deaka(raw);
        let n = counts[tid];
        if n < 4 {
            p.assign(p.idx + n, tid, 1.0);
            counts[tid] = n + 1;
        }
        if is_aka_id(raw) {
            p.fill(p.idx + 4 + aka_slot(raw), 1.0);
        }
    }
    p.advance(TILE_SET_CHANNELS);
}

/// `encode_self_kawa`: kan / tile / aka / dora.
fn encode_self_kawa(p: &mut Plane, item: Option<&KawaItem>) {
    if let Some(k) = item {
        for kan in k.kan.iter().flatten() {
            p.assign(p.idx, deaka(*kan), 1.0);
        }
        p.assign(p.idx + 1, k.tile, 1.0);
        if k.is_aka {
            p.fill(p.idx + 2, 1.0);
        }
        if k.is_dora {
            p.fill(p.idx + 3, 1.0);
        }
    }
    p.advance(SELF_KAWA_ITEM_CHANNELS);
}

/// `encode_kawa`: 8 channels for the other seats' discards.
///
/// Layout taken verbatim from `libriichi`'s `encode_kawa` (identical in the
/// 4-player and sanma sources):
///
/// ```text
///   +0  chi/pon consumed tile, lower
///   +1  chi/pon consumed tile, higher
///   +2  kan tiles
///   +3  the discarded tile
///   +4  aka (broadcast)
///   +5  is_dora (broadcast)
///   +6  is_tedashi (broadcast)
///   +7  is_riichi (broadcast)
/// ```
///
/// Note there is no tsumogiri channel: `is_tedashi` carries that information,
/// and the last channel is riichi.
fn encode_kawa(p: &mut Plane, item: Option<&KawaItem>) {
    if let Some(k) = item {
        if let Some((a, b)) = k.chi_pon {
            p.assign(p.idx, a.min(b), 1.0);
            p.assign(p.idx + 1, a.max(b), 1.0);
        }
        for kan in k.kan.iter().flatten() {
            p.assign(p.idx + 2, deaka(*kan), 1.0);
        }
        p.assign(p.idx + 3, k.tile, 1.0);
        if k.is_aka {
            p.fill(p.idx + 4, 1.0);
        }
        if k.is_dora {
            p.fill(p.idx + 5, 1.0);
        }
        if k.is_tedashi {
            p.fill(p.idx + 6, 1.0);
        }
        if k.is_riichi {
            p.fill(p.idx + 7, 1.0);
        }
    }
    p.advance(KAWA_ITEM_CHANNELS);
}

/// One discard slot, in the shape `encode_self_kawa` / `encode_kawa` want.
#[derive(Debug, Clone, Copy, Default)]
pub struct KawaItem {
    pub tile: usize,
    pub is_aka: bool,
    pub is_riichi: bool,
    pub is_tedashi: bool,
    pub is_tsumogiri: bool,
    pub is_dora: bool,
    /// Tiles of a chi/pon called off this discard, de-aka'd.
    pub chi_pon: Option<(usize, usize)>,
    /// Kan tiles, if this slot holds a kan.
    pub kan: [Option<u8>; 4],
}

fn deaka(tile: u8) -> usize {
    (tile / 4) as usize
}

/// Public alias of [`deaka`] for sibling modules.
#[inline]
pub fn mortal_obs_deaka(tile: u8) -> usize {
    deaka(tile)
}

/// Public alias of [`is_aka_id`] for sibling modules.
#[inline]
pub fn mortal_obs_is_aka(tile: u8) -> bool {
    is_aka_id(tile)
}

fn is_aka_id(tile: u8) -> bool {
    matches!(tile, 16 | 52 | 88)
}

fn aka_slot(tile: u8) -> usize {
    match tile {
        16 => 0,
        52 => 1,
        _ => 2,
    }
}

/// Everything the encoder reads, already rotated into self-relative seat order.
///
/// Built from a [`GameState`] by [`MortalSeatView::from_state`], which keeps
/// the rotation logic in one place and testable.
pub fn encode_mortal_obs(view: &MortalSeatView) -> Vec<f32> {
    encode_mortal_obs_with(view, Layout::V4)
}

/// Encode using an explicit [`Layout`].
pub fn encode_mortal_obs_with(view: &MortalSeatView, layout: Layout) -> Vec<f32> {
    let np = layout.num_players;
    let mut p = Plane::new(layout.channels);

    let tehai = &view.tehai;
    // -- self hand thresholds -------------------------------------------
    for (tile, &count) in tehai.iter().enumerate() {
        if count > 0 {
            p.assign_rows(p.idx, tile, (count as usize).min(4), 1.0);
        }
    }
    p.advance(4);
    // -- aka fives held --------------------------------------------------
    for (i, has) in view.akas_in_hand.iter().enumerate() {
        if *has {
            p.fill(p.idx + i, 1.0);
        }
    }
    p.advance(3);
    // -- score, two planes per player -----------------------------------
    for &score in view.scores.iter().take(np) {
        p.fill(p.idx, (score.clamp(0, layout.score_div[0] as i32) as f32) / layout.score_div[0]);
        p.advance(1);
        p.fill(p.idx, (score.clamp(0, layout.score_div[1] as i32) as f32) / layout.score_div[1]);
        p.advance(1);
    }
    // -- rank / kyoku one-hot --------------------------------------------
    p.fill(p.idx + view.rank.min(layout.rank_channels - 1), 1.0);
    p.advance(layout.rank_channels);
    p.fill(p.idx + (view.kyoku as usize).min(layout.kyoku_channels - 1), 1.0);
    p.advance(layout.kyoku_channels);
    // -- honba / kyotaku (v4: rescale only) -------------------------------
    p.fill(p.idx, view.honba.min(HONBA_CAP as u8) as f32 / HONBA_CAP as f32);
    p.advance(1);
    p.fill(p.idx, view.kyotaku.min(HONBA_CAP as u32) as f32 / HONBA_CAP as f32);
    p.advance(1);
    // -- round / seat wind ------------------------------------------------
    p.assign(p.idx, 27 + (view.round_wind as usize).min(3), 1.0);
    p.assign(p.idx + 1, 27 + (view.seat_wind as usize).min(3), 1.0);
    p.advance(2);
    // -- round progress ---------------------------------------------------
    let n = (view.round_wind as usize).min(1) * 4 + view.kyoku as usize;
    p.fill(p.idx, n.min(7) as f32 / 7.0);
    p.advance(1);
    // -- dora indicators ---------------------------------------------------
    encode_tile_set(&mut p, &view.dora_indicators);
    // -- self kawa: first 6, then last 18 ---------------------------------
    let self_kawa = &view.kawa[0];
    for item in self_kawa.iter().take(6) {
        encode_self_kawa(&mut p, item.as_ref());
    }
    p.advance((6 - self_kawa.len().min(6)) * SELF_KAWA_ITEM_CHANNELS);

    for item in self_kawa.iter().rev().take(18) {
        encode_self_kawa(&mut p, item.as_ref());
    }
    p.advance((18 - self_kawa.len().min(18)) * SELF_KAWA_ITEM_CHANNELS);

    let max_kawa_len = view.kawa.iter().map(|k| k.len()).max().unwrap_or(0);
    let alpha = |turn: usize| (-0.2 * (max_kawa_len.saturating_sub(1 + turn)) as f32).exp();
    // -- self kawa alpha-weighted -----------------------------------------
    for (turn, item) in self_kawa.iter().enumerate() {
        if let Some(k) = item {
            p.assign(p.idx, k.tile, alpha(turn));
        }
    }
    p.advance(1);
    // -- other seats' kawa -------------------------------------------------
    for seat_kawa in view.kawa.iter().skip(1).take(np - 1) {
        for item in seat_kawa.iter().take(6) {
            encode_kawa(&mut p, item.as_ref());
        }
        p.advance((6 - seat_kawa.len().min(6)) * KAWA_ITEM_CHANNELS);

        for item in seat_kawa.iter().rev().take(18) {
            encode_kawa(&mut p, item.as_ref());
        }
        p.advance((18 - seat_kawa.len().min(18)) * KAWA_ITEM_CHANNELS);

        for (turn, item) in seat_kawa.iter().enumerate() {
            if let Some(k) = item {
                let v = alpha(turn);
                p.assign(p.idx, k.tile, v);
                if k.is_tedashi {
                    p.assign(p.idx + 1, k.tile, v);
                }
                if k.is_riichi {
                    p.assign(p.idx + 2, k.tile, v);
                }
            }
        }
        p.advance(3);
    }
    // -- tiles left ---------------------------------------------------------
    p.fill(p.idx, view.tiles_left as f32 / layout.tiles_left_div);
    p.advance(1);
    // -- doras owned (v4: rescale /12) --------------------------------------
    for &count in view.doras_owned.iter().take(layout.doras_owned_channels) {
        p.fill(p.idx, count.min(12) as f32 / 12.0);
        p.advance(1);
    }
    // -- doras unseen (v4: rescale /23) -------------------------------------
    let unseen = view.dora_indicators.len().saturating_mul(4) + 3;
    let unseen = unseen.saturating_sub(view.doras_seen as usize);
    p.fill(p.idx, unseen.min(23) as f32 / layout.doras_unseen_div);
    p.advance(1);
    // -- kawa overview -------------------------------------------------------
    for overview in view.kawa_overview.iter().take(np) {
        encode_tile_set(&mut p, overview);
    }
    // -- fuuro overview: 4 sets x 5 channels per player ----------------------
    for fuuro in view.fuuro_overview.iter().take(np) {
        for meld in fuuro.iter().take(4) {
            for &raw in &meld.tiles {
                let tid = deaka(raw);
                let slot = (0..4).find(|&i| p.buf[(p.idx + i) * TILES + tid] == 0.0);
                if let Some(i) = slot {
                    p.assign(p.idx + i, tid, 1.0);
                }
                if is_aka_id(raw) {
                    p.fill(p.idx + 4, 1.0);
                }
            }
            p.advance(5);
        }
        p.advance((4 - fuuro.len().min(4)) * 5);
    }
    // -- ankan overview -------------------------------------------------------
    for kan in view.ankan_overview.iter().take(np) {
        for &raw in kan {
            p.assign(p.idx, deaka(raw), 1.0);
        }
        p.advance(1);
    }
    // -- tiles seen ------------------------------------------------------------
    for tile in 0..TILES {
        p.assign(p.idx, tile, view.tiles_seen[tile].min(4) as f32 / 4.0);
    }
    p.advance(1);
    // -- other seats' last tedashi (tile / aka / dora) --------------------------
    for item in view.last_tedashis.iter().skip(1).take(np - 1) {
        if let Some(k) = item {
            p.assign(p.idx, k.tile, 1.0);
            if k.is_aka {
                p.fill(p.idx + 1, 1.0);
            }
            if k.is_dora {
                p.fill(p.idx + 2, 1.0);
            }
        }
        p.advance(3);
    }
    // -- other seats' riichi sutehai --------------------------------------------
    for item in view.riichi_sutehais.iter().skip(1).take(np - 1) {
        if let Some(k) = item {
            p.assign(p.idx, k.tile, 1.0);
            if k.is_aka {
                p.fill(p.idx + 1, 1.0);
            }
            if k.is_dora {
                p.fill(p.idx + 2, 1.0);
            }
        }
        p.advance(3);
    }
    // -- other seats' riichi declared / accepted ---------------------------------
    for (i, &b) in view.riichi_declared.iter().skip(1).enumerate() {
        if b {
            p.fill(p.idx + i, 1.0);
        }
    }
    p.advance(layout.riichi_declared_channels);
    for (i, &b) in view.riichi_accepted.iter().skip(1).enumerate() {
        if b {
            p.fill(p.idx + i, 1.0);
        }
    }
    p.advance(layout.riichi_accepted_channels);
    // -- self waits ---------------------------------------------------------------
    for (tile, &w) in view.waits.iter().enumerate() {
        if w {
            p.assign(p.idx, tile, 1.0);
        }
    }
    p.advance(1);
    // -- furiten -------------------------------------------------------------------
    if view.at_furiten {
        p.fill(p.idx, 1.0);
    }
    p.advance(1);
    // -- shanten one-hot (cap 6 -> 7) ------------------------------------------------
    p.fill(p.idx + (view.shanten as usize).min(SHANTEN_CAP), 1.0);
    p.advance(SHANTEN_CAP + 1);
    // -- self riichi accepted / at kan select -----------------------------------------
    if view.riichi_accepted[0] {
        p.fill(p.idx, 1.0);
    }
    p.advance(1);
    if view.at_kan_select {
        p.fill(p.idx, 1.0);
    }
    p.advance(1);
    // -- call context ------------------------------------------------------------------
    if view.can_pass {
        if let Some(k) = view.last_kawa_tile {
            p.assign(p.idx, k.tile, 1.0);
            if k.is_aka {
                p.fill(p.idx + 1, 1.0);
            }
            if view.dora_factor[k.tile] > 0 {
                p.fill(p.idx + 2, 1.0);
            }
        }
    }
    p.advance(3);
    // -- decision flags ------------------------------------------------------------------
    //
    // The `*_candidates` fields hold RAW tile ids (0..135), matching the rest of
    // this crate and `Action::tile`; the reference implementation uses de-aka'd
    // 0..33 indices, so `deaka` is applied here.
    if view.can_discard {
        for &t in view.discard_candidates.iter() {
            p.assign(p.idx, deaka(t), 1.0);
        }
        for &t in view.keep_shanten_discards.iter() {
            p.assign(p.idx + 1, deaka(t), 1.0);
        }
        for &t in view.next_shanten_discards.iter() {
            p.assign(p.idx + 2, deaka(t), 1.0);
        }
        if view.shanten <= 1 {
            for &t in view.unconditional_tenpai_discards.iter() {
                p.assign(p.idx + 3, deaka(t), 1.0);
            }
        }
        if view.riichi_declared[0] {
            p.fill(p.idx + 4, 1.0);
        }
    }
    p.advance(5);

    if view.can_riichi {
        p.fill(p.idx, 1.0);
    }
    p.advance(1);
    // 4-player: chi low/mid/high, pon, daiminkan.
    // Akagi's 775-channel sanma: pon, daiminkan, nukidora (in that order).
    if layout.call_block_channels == 3 {
        if view.can_chi_low {
            p.fill(p.idx, 1.0);
        }
        if view.can_chi_mid {
            p.fill(p.idx + 1, 1.0);
        }
        if view.can_chi_high {
            p.fill(p.idx + 2, 1.0);
        }
        if view.can_pon { p.fill(p.idx + 3, 1.0); }
        if view.can_daiminkan { p.fill(p.idx + 4, 1.0); }
    } else {
        if view.can_pon { p.fill(p.idx, 1.0); }
        if view.can_daiminkan { p.fill(p.idx + 1, 1.0); }
        if view.can_nukidora { p.fill(p.idx + 2, 1.0); }
    }
    p.advance(layout.call_block_channels + 2);
    if view.can_ankan {
        for &t in view.ankan_candidates.iter() {
            p.assign(p.idx, deaka(t), 1.0);
        }
    }
    p.advance(1);
    if view.can_kakan {
        for &t in view.kakan_candidates.iter() {
            p.assign(p.idx, deaka(t), 1.0);
        }
    }
    p.advance(1);
    if view.can_agari {
        p.fill(p.idx, 1.0);
    }
    p.advance(1);
    if view.can_ryukyoku {
        p.fill(p.idx, 1.0);
    }
    p.advance(1);
    // -- v4 single-player-table tail -------------------------------------------------------
    p.fill(p.idx, (view.max_ev.clamp(0.0, layout.score_div[0])) / layout.score_div[0]);
    p.fill(p.idx + 1, (view.max_ev.clamp(0.0, layout.score_div[1])) / layout.score_div[1]);
    p.advance(2);

    if view.can_discard {
        for cand in view.sp_candidates.iter() {
            for &req in cand.required_tiles.iter() {
                let dst = if cand.shanten_down {
                    p.idx + TILES + deaka(cand.tile)
                } else {
                    p.idx + deaka(cand.tile)
                };
                p.assign(dst, deaka(req), 1.0);
            }
        }
        p.advance(2 * TILES);

        if let Some(best) = view
            .sp_candidates
            .iter()
            .max_by_key(|c| (!c.shanten_down, c.required_tiles.len()))
        {
            p.assign(p.idx, deaka(best.tile), 1.0);
        }
        p.advance(2);

        encode_sp_table(&mut p, view, true);
    } else {
        p.advance(2 * TILES + 1);
        if let Some(first) = view.sp_candidates.first() {
            for &req in first.required_tiles.iter() {
                p.assign(p.idx, deaka(req), 1.0);
            }
        }
        p.advance(1);
        encode_sp_table(&mut p, view, false);
    }

    debug_assert_eq!(p.idx, layout.channels, "mortal obs channel cursor drift");
    p.buf
}

/// `encode_sp_table`: 3 planes of `MAX_NUM_TURNS`, keyed by discard tile.
fn encode_sp_table(p: &mut Plane, view: &MortalSeatView, can_discard: bool) {
    let has_probs = |c: &SpCandidate| c.tenpai_probs.first().is_some_and(|&x| x > 0.0);
    let first = view.sp_candidates.iter().find(|c| has_probs(c));

    let Some(first) = first else {
        p.advance(3 * MAX_NUM_TURNS);
        return;
    };

    let ev_scale = if view.max_ev < 1.0 { 0.0 } else { 1.0 / view.max_ev };

    if can_discard {
        for cand in view.sp_candidates.iter() {
            let tid = deaka(cand.tile);
            for turn in 0..MAX_NUM_TURNS {
                let Some(&tp) = cand.tenpai_probs.get(turn) else {
                    break;
                };
                if tp <= 0.0 {
                    break;
                }
                let wp = cand.win_probs.get(turn).copied().unwrap_or(0.0);
                let ev = cand.exp_values.get(turn).copied().unwrap_or(0.0);
                p.assign(p.idx + turn, tid, tp);
                p.assign(p.idx + MAX_NUM_TURNS + turn, tid, wp);
                p.assign(p.idx + 2 * MAX_NUM_TURNS + turn, tid, (ev * ev_scale).min(1.0));
            }
        }
    } else {
        for turn in 0..MAX_NUM_TURNS {
            let Some(&tp) = first.tenpai_probs.get(turn) else {
                break;
            };
            if tp <= 0.0 {
                break;
            }
            let wp = first.win_probs.get(turn).copied().unwrap_or(0.0);
            let ev = first.exp_values.get(turn).copied().unwrap_or(0.0);
            p.fill(p.idx + turn, tp);
            p.fill(p.idx + MAX_NUM_TURNS + turn, wp);
            p.fill(p.idx + 2 * MAX_NUM_TURNS + turn, (ev * ev_scale).min(1.0));
        }
    }
    p.advance(3 * MAX_NUM_TURNS);
}

/// One single-player-table row.
#[derive(Debug, Clone, Default)]
pub struct SpCandidate {
    pub tile: u8,
    pub shanten_down: bool,
    pub tenpai_probs: Vec<f32>,
    pub win_probs: Vec<f32>,
    pub exp_values: Vec<f32>,
    pub required_tiles: Vec<u8>,
}

/// Build the self-relative kawa for one seat from its discard record.
///
/// `discards[i]` is the raw tile id, `from_hand[i]` says whether it was a
/// tedashi (as opposed to tsumogiri), and `is_riichi[i]` marks the riichi
/// declaration discard.
///
/// The `is_dora` channel is left false: whether a discarded tile is a dora is a
/// property of the *round's* dora indicators, not of the discard, so the caller
/// sets it via [`build_kawa_with_dora`] when it has that context.
pub fn build_kawa(discards: &[u8], from_hand: &[bool], is_riichi: &[bool]) -> Vec<Option<KawaItem>> {
    let dora = vec![false; discards.len()];
    build_kawa_with_dora(discards, from_hand, is_riichi, &dora)
}

/// [`build_kawa`], plus a per-discard `is_dora` flag.
pub fn build_kawa_with_dora(
    discards: &[u8],
    from_hand: &[bool],
    is_riichi: &[bool],
    is_dora: &[bool],
) -> Vec<Option<KawaItem>> {
    discards
        .iter()
        .enumerate()
        .map(|(i, &raw)| {
            // Default to "not tedashi" when the caller has no per-discard
            // information: riichienv's 3-player state never records it.
            let tedashi = from_hand.get(i).copied().unwrap_or(false);
            Some(KawaItem {
                tile: deaka(raw),
                is_aka: is_aka_id(raw),
                is_riichi: is_riichi.get(i).copied().unwrap_or(false),
                is_tedashi: tedashi,
                is_tsumogiri: !tedashi,
                is_dora: is_dora.get(i).copied().unwrap_or(false),
                chi_pon: None,
                kan: [None; 4],
            })
        })
        .collect()
}

/// Split a seat's melds into (chi/pon/daiminkan for fuuro overview, ankan).
pub fn split_melds(melds: &[Meld]) -> (Vec<Meld>, Vec<Vec<u8>>) {
    let mut fuuro = Vec::new();
    let mut ankan = Vec::new();
    for m in melds {
        match m.meld_type {
            MeldType::Ankan => ankan.push(m.tiles.clone()),
            _ => fuuro.push(m.clone()),
        }
    }
    (fuuro, ankan)
}

/// Convenience: encode directly from a [`GameState`] for `seat`.
pub fn encode_mortal_obs_4p_state(state: &GameState, seat: u8) -> Vec<f32> {
    let view = MortalSeatView::from_state(state, seat);
    encode_mortal_obs(&view)
}

/// The channel budget the layout above consumes, without touching a game state.
///
/// Mirrors `mortal_obs_ref.py::budget`. Kept as a test so a change to the
/// encoder has to keep the cursor on exactly 1012.
#[cfg(test)]
mod budget_tests {
    use super::*;

    fn ie(cap: usize, one_hot: bool, rescale: bool) -> usize {
        (if one_hot { cap + 1 } else { 0 }) + (if rescale { 1 } else { 0 })
    }

    /// Sum of the sections in `tools/MORTAL_OBS_PARITY.md`, for version 4.
    fn expected_budget() -> usize {
        let mut n = 0;
        n += 4 + 3 + 4 * 2 + 4 + 4;
        n += ie(HONBA_CAP, false, true) * 2;
        n += 2;
        n += ie(7, false, true);
        n += TILE_SET_CHANNELS;
        n += 6 * SELF_KAWA_ITEM_CHANNELS + 18 * SELF_KAWA_ITEM_CHANNELS + 1;
        n += 3 * (6 * KAWA_ITEM_CHANNELS + 18 * KAWA_ITEM_CHANNELS + 3);
        n += 1;
        n += 4 * ie(12, false, true);
        n += ie(23, false, true);
        n += 4 * TILE_SET_CHANNELS;
        n += 4 * 4 * 5;
        n += 4 + 1;
        n += 3 * 3 + 3 * 3;
        n += 3 + 3;
        n += 1 + 1;
        n += ie(SHANTEN_CAP, true, false);
        n += 1 + 1 + 3;
        n += 5 + 1 + 3 + 1 + 1 + 1 + 1 + 1 + 1;
        n += 2 * TILES + 2 + 3 * MAX_NUM_TURNS + 2;
        n
    }

    #[test]
    fn budget_equals_the_libriichi_shape() {
        assert_eq!(
            expected_budget(),
            CHANNELS_V4,
            "section budget disagrees with libriichi obs_shape(4)"
        );
    }

    /// The 3-player budget, with the section boundaries that were measured
    /// directly off ground-truth `libriichi3p` buffers (`tools/sanma/README.md`).
    ///
    /// Two independent checks: the total must be `obs_shape(4) = 775` for that
    /// build, and the anchors observed in the real data must land where the
    /// encoder puts them.
    #[test]
    fn sanma_budget_and_anchors_match_libriichi3p() {
        let l = Layout::SANMA;
        let np = l.num_players;

        let mut n = 0usize;
        macro_rules! anchor {
            ($name:expr, $want:expr) => {
                assert_eq!(
                    n, $want,
                    "sanma anchor `{}` expected at {}, got {}",
                    $name, $want, n
                )
            };
        }

        n += 4 + 3 + np * 2; // tehai, akas, scores
        anchor!("rank", 13);
        n += l.rank_channels;
        anchor!("kyoku", 16);
        n += l.kyoku_channels;
        n += 1 + 1 + 2 + 1; // honba, kyotaku, winds, round progress
        n += TILE_SET_CHANNELS;
        anchor!("self kawa", 31);
        n += 6 * SELF_KAWA_ITEM_CHANNELS + 18 * SELF_KAWA_ITEM_CHANNELS + 1;
        anchor!("other kawas", 128);
        n += (np - 1) * (6 * KAWA_ITEM_CHANNELS + 18 * KAWA_ITEM_CHANNELS + 3);
        anchor!("tiles_left", 518);
        n += 1;
        anchor!("doras_owned", 519);
        n += l.doras_owned_channels;
        anchor!("doras_unseen", 523);
        n += 1;
        anchor!("kawa overview", 524);
        n += np * TILE_SET_CHANNELS;
        anchor!("fuuro overview", 545);
        n += np * 4 * 5;
        anchor!("ankan overview", 605);
        n += np;
        anchor!("tiles_seen", 608);
        n += 1;
        anchor!("last tedashis", 609);
        n += (np - 1) * 3;
        anchor!("riichi sutehais", 615);
        n += (np - 1) * 3;
        anchor!("riichi declared", 621);
        n += l.riichi_declared_channels;
        anchor!("riichi accepted", 623);
        n += l.riichi_accepted_channels;
        anchor!("waits", 625);
        n += 1;
        anchor!("furiten", 626);
        n += 1;
        anchor!("shanten", 627);
        n += SHANTEN_CAP + 1;
        anchor!("self riichi accepted", 634);
        n += 1;
        anchor!("at_kan_select", 635);
        n += 1;
        anchor!("call context", 636);
        n += 3;
        // can_discard(5) + can_riichi(1) + nukidora(1) + pon/daiminkan/
        // ankan/kakan/agari/ryukyoku(6) = 13. The 4p layout spends 3 on chi
        // instead of 1 on nukidora, and that 2-channel difference plus the
        // rank/kyoku/riichi widths is what makes this 775 rather than 780.
        anchor!("decision flags", 639);
        n += 5 + 1 + l.call_block_channels + 6;
        anchor!("encode_ev", 652);
        n += 2;
        anchor!("required tiles", 654);
        n += 2 * TILES + 2;
        anchor!("single-player table", 724);
        n += 3 * MAX_NUM_TURNS;

        assert_eq!(
            n, l.channels,
            "sanma budget is {n}, libriichi3p reports obs_shape(4).0 = {}",
            l.channels
        );
    }

    /// The encoder must actually consume the budget, not just almost all of it.
    #[test]
    fn encoder_cursor_lands_on_the_budget() {
        let view = crate::state_features::MortalSeatView::default();
        let obs = encode_mortal_obs(&view);
        assert_eq!(obs.len(), expected_budget() * TILES);
    }

    /// The 3-player encoder must emit exactly 775 x 34 and stay in [0, 1].
    #[test]
    fn sanma_encoder_emits_the_full_buffer() {
        let mut view = crate::state_features::MortalSeatView::default();
        view.n_players = 3;
        let obs = encode_mortal_obs_with(&view, Layout::SANMA);
        assert_eq!(obs.len(), CHANNELS_SANMA * TILES);
        assert!(
            obs.iter().all(|v| (0.0..=1.0).contains(v)),
            "sanma encoder produced values outside [0, 1]"
        );
    }
}
