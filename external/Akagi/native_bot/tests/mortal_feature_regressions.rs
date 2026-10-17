
use native_bot::state_features::MortalSeatView;
use riichienv_core::{state::GameState,rule::GameRule};
fn state() -> GameState { GameState::new(0,true,None,0,GameRule::default()) }
#[test]
fn shanten() {
 let mut st=state();st.players[0].hand=vec![0,16,32,36,52,68,76,88,100,108,112,116,120];
 let v=MortalSeatView::from_state(&st,0);
 eprintln!("shanten actual={} expected=5",v.shanten);
 assert_eq!(v.shanten,5);
}
#[test]
fn manzu_dora() {
 let mut st=state();st.players[0].hand=vec![0,4,32,36,52,68,76,88,100,108,112,116,120];
 st.wall.dora_indicators=vec![0];
 let v=MortalSeatView::from_state(&st,0);
 eprintln!("1m indicator: 2m factor={} 2p factor={} owned={}",v.dora_factor[1],v.dora_factor[10],v.doras_owned[0]);
 assert_eq!(v.dora_factor[1],1);
 assert_eq!(v.dora_factor[10],0);
 assert_eq!(v.doras_owned[0],1);
}
#[test]
fn permanent_furiten() {
 let mut st=state();st.players[0].hand=vec![0,1,2,36,37,38,72,73,74,108,109,110,53];
 st.players[0].discards=vec![54];
 let v=MortalSeatView::from_state(&st,0).with_legal_actions(&mut st,0);
 eprintln!("5p single wait={} furiten={}",v.waits[13],v.at_furiten);
 assert!(v.waits[13]);
 assert!(v.at_furiten);
}
