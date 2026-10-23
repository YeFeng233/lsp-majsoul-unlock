//! Run a user-supplied sanma checkpoint through the complete MJAI -> policy path.
//! No weights are distributed. Usage: cargo run --example mortal_sanma_smoke -- model.pth
use std::sync::{OnceLock, atomic::{AtomicBool, Ordering}};
use native_bot::{defaults, engine::BotAction, mortal};

static CHECKPOINT: OnceLock<String> = OnceLock::new();
static WRONG_CALLBACK: AtomicBool = AtomicBool::new(false);

unsafe extern "C" fn activate(players: u8) -> i32 {
    if players != 3 { return -1; }
    if mortal::load_pth(3, CHECKPOINT.get().unwrap(), true) == 3 { 0 } else { -1 }
}

unsafe extern "C" fn unexpected_akagi_callback(_: u8, _: *const f32, _: usize, _: *mut f32, _: usize) -> i32 {
    // A valid PTH contract runs locally; a callback here indicates wrong routing.
    WRONG_CALLBACK.store(true, Ordering::Relaxed);
    -1
}

fn main() -> anyhow::Result<()> {
    CHECKPOINT.set(std::env::args().nth(1).expect("pass a sanma checkpoint path")).unwrap();
    let mut engine = defaults::engine_with_external(3, 0, unexpected_akagi_callback, activate)?;
    anyhow::ensure!(engine.uses_experimental_policy(), "sanma model was not activated");
    let hand = r#"["1m","9m","1p","2p","3p","4p","5p","6p","7p","8p","9p","N","N"]"#;
    anyhow::ensure!(engine.feed_line(r#"{"type":"start_game","names":["a","b","c",""]}"#));
    anyhow::ensure!(engine.feed_line(&format!(
        r#"{{"type":"start_kyoku","bakaze":"E","dora_marker":"1s","kyoku":1,"honba":0,
        "kyotaku":0,"oya":0,"scores":[35000,35000,35000,0],"tehais":[{hand},{hand},{hand},{hand}]}}"#
    )));
    anyhow::ensure!(engine.feed_line(r#"{"type":"tsumo","actor":0,"pai":"9p"}"#));
    let decision = engine.decide()?.expect("our draw must produce a decision");
    anyhow::ensure!(!decision.custom_fallback, "unexpected bundled-model fallback");
    anyhow::ensure!(!WRONG_CALLBACK.load(Ordering::Relaxed), "Mortal PTH was routed through the Akagi callback");
    anyhow::ensure!(decision.logits.len() == 44 && decision.logits.iter().all(|v| v.is_finite()));
    anyhow::ensure!(!decision.candidates.is_empty());
    for (action, probability) in &decision.candidates {
        anyhow::ensure!(probability.is_finite() && (0.0..=1.0).contains(probability));
        if let BotAction::Dahai { pai, .. } = action {
            anyhow::ensure!(!["2m","3m","4m","5m","5mr","6m","7m","8m"].contains(&pai.as_str()));
        }
        println!("{action:?}: {probability:.6}");
    }
    println!("PASS sanma MJAI observation, 44-action inference and legal ranking; no fallback");
    Ok(())
}
