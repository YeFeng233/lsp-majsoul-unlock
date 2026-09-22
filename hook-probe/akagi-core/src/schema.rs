#[path = "../../../external/Akagi/src/schema/mjai/mod.rs"]
pub mod mjai;
#[path = "../../../external/Akagi/src/schema/history.rs"]
pub mod history;
pub use mjai::{GameEndReason, GameMeta, MjaiEvent};
pub use history::MatchInfo;

#[derive(Debug, Clone)]
pub struct ParsedFrame { pub method: String, pub args: serde_json::Value }
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HoraScoreInfo {
    pub points: u32,
    pub han: u32,
    pub fu: u32,
    pub yakuman: bool,
    pub win_tile: String,
}
