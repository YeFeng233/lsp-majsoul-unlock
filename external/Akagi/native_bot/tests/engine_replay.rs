//! Exercise the full inference path (feed mjai events -> riichienv state ->
//! obs encode -> candle forward -> decode to a legal action) on a small,
//! self-contained mjai game. Portable: no dataset dependency.
#![cfg(feature = "infer")]

use std::fs;
use std::path::Path;

use native_bot::engine::{BotAction, Engine};
use riichienv_core::replay::MjaiEvent;

fn weights(name: &str) -> Vec<u8> {
    fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("weights")
            .join(name),
    )
    .expect("weights")
}

fn ev(line: &str) -> MjaiEvent {
    serde_json::from_str(line).expect("valid mjai event")
}

#[test]
fn decides_a_legal_discard_after_tsumo() {
    let hand = r#"["1m","2m","3m","4m","5m","6m","7m","8m","9m","1p","2p","3p","4p"]"#;
    let start_kyoku = format!(
        r#"{{"type":"start_kyoku","bakaze":"E","dora_marker":"2m","kyoku":1,"honba":0,"kyotaku":0,"oya":0,"scores":[25000,25000,25000,25000],"tehais":[{hand},{hand},{hand},{hand}]}}"#
    );

    let mut eng = Engine::new(weights("akagi4p.safetensors"), 4, 0).unwrap();
    eng.feed(ev(r#"{"type":"start_game","names":["a","b","c","d"]}"#));
    eng.feed(ev(&start_kyoku));
    eng.feed(ev(r#"{"type":"tsumo","actor":0,"pai":"5p"}"#));

    let decision = eng
        .decide()
        .unwrap()
        .expect("we should have a decision on our tsumo");
    // Our turn after a draw: the only free choices are discard / riichi / kan /
    // hora / kyushu. It must never be a claim/pass here.
    assert!(
        matches!(
            decision.action,
            BotAction::Dahai { .. }
                | BotAction::Reach { .. }
                | BotAction::Ankan { .. }
                | BotAction::Kakan { .. }
                | BotAction::Hora { .. }
                | BotAction::Kyushu
        ),
        "unexpected action on own turn: {:?}",
        decision.action
    );
    // A fresh 14-tile hand has many legal discards — nothing forced about it.
    assert!(
        !decision.forced,
        "an opening discard choice must not be flagged forced"
    );
    assert_eq!(decision.logits.len(), 82);
    assert!(
        decision.logits.iter().all(|v| v.is_finite()),
        "logits finite"
    );
}

#[test]
fn plays_several_turns_without_panic() {
    let hand = r#"["1m","2m","3m","4m","5m","6m","7m","8m","9m","1p","2p","3p","4p"]"#;
    let start_kyoku = format!(
        r#"{{"type":"start_kyoku","bakaze":"E","dora_marker":"2m","kyoku":1,"honba":0,"kyotaku":0,"oya":0,"scores":[25000,25000,25000,25000],"tehais":[{hand},{hand},{hand},{hand}]}}"#
    );
    let mut eng = Engine::new(weights("akagi4p.safetensors"), 4, 0).unwrap();
    eng.feed(ev(r#"{"type":"start_game","names":["a","b","c","d"]}"#));
    eng.feed(ev(&start_kyoku));

    // A few rounds of the table: seat 0 draws + we decide; others draw+discard.
    for turn in 0..4 {
        eng.feed(ev(r#"{"type":"tsumo","actor":0,"pai":"5s"}"#));
        let d = eng.decide().unwrap().expect("decision on our tsumo");
        // Apply our own discard back so the engine advances like a real game.
        if let BotAction::Dahai { pai, .. } = &d.action {
            eng.feed(ev(&format!(
                r#"{{"type":"dahai","actor":0,"pai":"{pai}","tsumogiri":false}}"#
            )));
        }
        for seat in 1..4u8 {
            eng.feed(ev(&format!(
                r#"{{"type":"tsumo","actor":{seat},"pai":"9s"}}"#
            )));
            eng.feed(ev(&format!(
                r#"{{"type":"dahai","actor":{seat},"pai":"9s","tsumogiri":true}}"#
            )));
        }
        let _ = turn;
    }
}
