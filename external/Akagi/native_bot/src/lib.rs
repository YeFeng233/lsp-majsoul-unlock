//! `native_bot` — a built-in, libriichi-free mahjong bot for Akagi.
//!
//! This crate provides everything needed to run a small behavior-cloned neural
//! net entirely in Rust:
//! - [`obs`] — our own observation feature encoding (`EncInput` -> `[C, T]`).
//! - [`action_codec`] — action indexing / legal masking / logit decoding,
//!   reusing riichienv-core's canonical action ids.
//! - [`adapt`] — build an `EncInput` from a riichienv-core observation (shared
//!   by extraction and inference, guaranteeing feature parity).
//! - [`mjai_compat`] — event fixups applied before an mjai stream reaches a
//!   riichienv-core state (shared by extraction and inference).
//! - [`replay`] — drive an mjai game log through the engine, emitting
//!   `(obs, action, mask)` training samples (extractor side).
//! - [`chankan`] — open the chankan ron window that riichienv-core's mjai
//!   replay path skips (used by both live engines below).
//! - [`model`] / [`engine`] — candle CNN inference (behind the `infer` feature).

pub mod action_codec;
pub mod adapt;
pub mod chankan;
pub mod mjai_compat;
pub mod obs;
pub mod replay;
pub mod tiles;

#[cfg(feature = "infer")]
pub mod defaults;
#[cfg(feature = "infer")]
pub mod engine;
#[cfg(feature = "infer")]
pub mod model;

/// Feature-plane and action-space geometry for a given player count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    pub num_players: u8,
    pub channels: usize,
    pub tile_dim: usize,
    pub action_space: usize,
}

impl Geometry {
    pub fn for_players(num_players: u8) -> Self {
        Self {
            num_players,
            channels: obs::channels(num_players),
            tile_dim: tiles::tile_dim(num_players),
            action_space: action_codec::action_space(num_players),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_values() {
        let g4 = Geometry::for_players(4);
        assert_eq!(g4.tile_dim, 34);
        assert_eq!(g4.action_space, 82);
        let g3 = Geometry::for_players(3);
        assert_eq!(g3.tile_dim, 27);
        assert_eq!(g3.action_space, 60);
        assert!(g4.channels > 0 && g3.channels > 0);
    }
}
