//! Bundled default model weights + a convenience engine constructor.
//!
//! The trained `.safetensors` are embedded directly into the binary so the
//! built-in bot needs no external files at runtime.

use crate::engine::Engine;

/// 4-player weights (behavior-cloned from Tenhou logs).
pub const WEIGHTS_4P: &[u8] = include_bytes!("../weights/akagi4p.safetensors");
/// 3-player (sanma) weights.
pub const WEIGHTS_3P: &[u8] = include_bytes!("../weights/akagi3p.safetensors");

/// Build an [`Engine`] for the given player count, seated at `seat`, using the
/// bundled default weights.
pub fn engine(num_players: u8, seat: u8) -> anyhow::Result<Engine> {
    let bytes = if num_players == 3 {
        WEIGHTS_3P
    } else {
        WEIGHTS_4P
    };
    Engine::new(bytes.to_vec(), num_players, seat)
}
