//! Android adapter around pinned, unmodified Akagi analysis source.
//! Desktop capture, UI, cloud inference and automatic input are not linked.
pub mod bridge;
pub mod schema;
pub mod autoplay;

#[path = "../../../external/Akagi/src/analysis/mod.rs"]
pub mod analysis;
#[path = "../../../external/Akagi/src/game_state/mod.rs"]
pub mod game_state;

pub mod config {
    pub enum Platform { Majsoul }
    impl Platform {
        pub fn subdir(&self) -> &'static str { "majsoul" }
    }
}

// The bridge's optional desktop logging slots are always None in this app.
// Never persist raw packets, account credentials, or opponents' identities.
pub mod logger {
    pub struct FlowLogger;
    pub struct Session;
    impl FlowLogger { pub fn writeln(&self, _: &str) {} }
    impl Session {
        pub fn flow_logger(&self, _: &str, _: &str, _: String)
            -> anyhow::Result<std::sync::Arc<FlowLogger>> {
            anyhow::bail!("desktop packet logging is disabled on Android")
        }
    }
}

// Upstream tracker/runner APIs remain available, but the mobile host feeds
// events synchronously on one background worker to preserve ordering.
pub mod event_bus {
    use crate::schema::MjaiEvent;
    #[derive(Debug, Clone)]
    pub struct TrackedEvent { pub event: MjaiEvent, pub can_act: Option<bool> }
    pub type PostTrackerBus = tokio::sync::broadcast::Sender<TrackedEvent>;
    pub type AnalysisBus = tokio::sync::broadcast::Sender<crate::analysis::AnalysisResult>;
}
