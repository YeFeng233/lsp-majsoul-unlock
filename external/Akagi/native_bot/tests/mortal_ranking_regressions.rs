
use native_bot::mortal::rank_by_mortal_logits;
use native_bot::state_features::MortalSeatView;
use riichienv_core::{state::GameState, rule::GameRule, action::{Action, ActionType}};

#[test]
fn live_shanten_must_match_hand_count() {
 let mut st=GameState::new(0,true,None,0,GameRule::default());
 st.players[0].hand=vec![0,16,32,36,52,68,76,88,100,108,112,116,120];
 let view=MortalSeatView::from_state(&st,0);
 let mut counts=[0u8;34];
 for t in &st.players[0].hand {counts[(*t/4) as usize]+=1;}
 let want=riichienv_core::shanten::calc_shanten_from_counts(&counts,4).clamp(0,6) as u8;
 eprintln!("review: live shanten={} expected={}",view.shanten,want);
 assert_eq!(view.shanten,want);
}

#[test]
fn ranked_probabilities_must_follow_sorted_actions() {
 let a=Action::new(ActionType::Discard,Some(0),vec![],None);
 let b=Action::new(ActionType::Discard,Some(4),vec![],None);
 let mut logits=vec![0f32;46];logits[0]=1.;logits[1]=2.;
 let ranked=rank_by_mortal_logits(&[a,b],&logits,4,2);
 eprintln!("review: ranked probabilities={:?}",ranked.iter().map(|(a,p)|(a.tile,*p)).collect::<Vec<_>>());
 assert!(ranked[0].1>ranked[1].1);
}
