//! Live game-state tracking layered on top of `riichienv-core`.
//!
//! Subscribes to the MJAI event bus and maintains an in-memory
//! `GameState` mirror of the live game so other subsystems can ask
//! questions like "what's seat 2's hand?", "is the dealer in tenpai?",
//! "what's the score breakdown for han=3 fu=30?" without re-parsing
//! mjai logs.
//!
//! See `README.md` in this directory for the contributor-facing guide.

pub mod convert;
pub mod mahgen_view;
pub mod score;
pub mod snapshot;
pub mod tracker;

pub use mahgen_view::{MahgenView, PlayerMahgenView};
pub use score::{calculate_score, is_tenpai, waits_for, Score};
pub use snapshot::{
    DiscardEntry, GameStateSnapshot, MeldKind, MeldSnapshot, Phase, PlayerSnapshot,
};
pub use tracker::{spawn, spawn_with_post, GameTracker};
