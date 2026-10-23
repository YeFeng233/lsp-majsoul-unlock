//! Export the production sanma observation/mask for an independent replay audit.
use std::fs;
use native_bot::{mortal::{encode_mortal_obs_3p, mortal_action_index}, mjai_compat::parse_line};
use riichienv_core::{state_3p::GameState3P, rule::GameRule, action::{Action, ActionType}};
use serde_json::{Value, json};

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(args.len() == 3, "usage: mortal_observation_replay cases.json observations.json");
    let cases: Vec<Value> = serde_json::from_str(&fs::read_to_string(&args[1])?)?;
    let mut results = Vec::new();
    for case in cases {
        let seat = case["seat"].as_u64().unwrap() as u8;
        let mut state = GameState3P::new(0, false, None, 0, GameRule::default_tenhou());
        for event in case["events"].as_array().unwrap() {
            if let Some(event) = parse_line(&event.to_string(), 3) {
                native_bot::mjai_compat::apply_sanma_event(&mut state, event);
            }
        }
        let (obs, legal) = encode_mortal_obs_3p(&mut state, seat, 775);
        let mut mask = vec![false; 44];
        for action in &legal {
            if let Some(index) = mortal_action_index(action, 3) { mask[index] = true; }
        }
        let index = |kind| mortal_action_index(&Action::new(kind, None, vec![], None), 3);
        results.push(json!({"id":case["id"],"obs":obs,"mask":mask,
            "specialActionIndices":{"kita":index(ActionType::Kita),"pon":index(ActionType::Pon),"kan":index(ActionType::Ankan)}}));
    }
    fs::write(&args[2], serde_json::to_vec(&results)?)?;
    Ok(())
}
