use crate::schema::{MjaiEvent, ParsedFrame};
#[path = "../../../external/Akagi/src/bridge/majsoul/mod.rs"]
pub mod majsoul;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction { Up, Down }
impl Direction {
    pub fn as_str(self) -> &'static str {
        match self { Self::Up => "up", Self::Down => "down" }
    }
}
#[derive(Debug, Clone, Default)]
pub struct ParseResult {
    pub events: Vec<MjaiEvent>,
    pub parsed: Option<ParsedFrame>,
}
impl ParseResult { pub fn empty() -> Self { Self::default() } }
pub trait Bridge: Send {
    fn parse(&mut self, direction: Direction, content: &[u8]) -> ParseResult;
    fn build(&mut self, command: &MjaiEvent) -> Option<Vec<u8>>;
}
