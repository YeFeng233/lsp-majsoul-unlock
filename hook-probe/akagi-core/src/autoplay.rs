// These are observation-only budget/input-counter types required by the
// upstream bridge. No autoplay manager or game-input implementation is built.
#[path = "../../../external/Akagi/src/autoplay/budget.rs"]
pub mod budget;
#[path = "../../../external/Akagi/src/autoplay/verify.rs"]
pub mod verify;
pub use verify::InputKind;
