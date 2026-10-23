//! Selected independent libriichi3p planes. This is not a full feature-parity test.
#![cfg(feature = "infer")]
use native_bot::{mjai_compat::{apply_sanma_event, parse_line}, mortal::{encode_mortal_obs_3p, mortal_action_index}};
use riichienv_core::{state_3p::GameState3P, rule::GameRule};
use serde_json::Value;

#[test]
fn sanma_replays_match_independent_contract_and_selected_features() {
    let fixtures: Value = serde_json::from_str(include_str!("fixtures/sanma-reference.json")).unwrap();
    for case in fixtures["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let seat = case["seat"].as_u64().unwrap() as u8;
        let mut state = GameState3P::new(0, false, None, 0, GameRule::default_tenhou());
        for event in case["events"].as_array().unwrap() {
            if let Some(event) = parse_line(&event.to_string(), 3) { apply_sanma_event(&mut state, event); }
        }
        let (obs, legal) = encode_mortal_obs_3p(&mut state, seat, 775);
        let mut mask: Vec<_> = legal.iter().filter_map(|a| mortal_action_index(a, 3)).collect();
        mask.sort_unstable();mask.dedup();
        let expected_mask: Vec<_> = case["mask"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as usize).collect();
        assert_eq!(mask, expected_mask, "{id}: legal action indices");
        for (channel, expected) in case["features"].as_object().unwrap() {
            let channel: usize = channel.parse().unwrap();
            for tile in 0..34 {
                let value = expected.as_f64().unwrap_or_else(|| expected[tile].as_f64().unwrap()) as f32;
                assert!((obs[channel*34+tile] - value).abs() < 1e-6,
                    "{id}: channel {channel}, tile {tile}: {} != {value}",obs[channel*34+tile]);
            }
        }
    }
}
