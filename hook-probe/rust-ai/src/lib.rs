//! Single-worker Android host for Akagi's protocol bridge and local engines.
use akagi_mobile_core::{
    analysis::{self, snapshot_adapter::to_player_info},
    bridge::{majsoul::MajsoulBridge, Bridge, Direction},
    game_state::GameTracker,
    schema::MjaiEvent,
};
use native_bot::engine::{BotAction, Engine};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    ffi::{c_char, CString},
    panic::{catch_unwind, AssertUnwindSafe},
    slice,
    sync::{Mutex, OnceLock},
    time::Instant,
};

const MAX_FRAME: usize = 1024 * 1024;
static HOST: OnceLock<Mutex<Assistant>> = OnceLock::new();

struct Assistant {
    flows: HashMap<u64, MajsoulBridge>,
    active: Option<u64>,
    tracker: GameTracker,
    engine: Option<Engine>,
    round_ready: bool,
    input_pending: bool,
    revision: u64,
}

fn waiting(message: &str) -> Value {
    json!({"status":"waiting", "message":message, "recommendations":[]})
}

impl Assistant {
    fn new() -> Self {
        Self {
            flows: HashMap::new(), active: None, tracker: GameTracker::new(),
            engine: None, round_ready: false, input_pending: false, revision: 0,
        }
    }

    fn invalidate(&mut self, message: &str) -> Value {
        *self = Self::new();
        waiting(message)
    }

    fn frame(&mut self, connection: u64, direction: u8, data: &[u8]) -> anyhow::Result<Option<Value>> {
        if direction == 2 {
            self.flows.remove(&connection);
            return Ok(if self.active == Some(connection) {
                Some(self.invalidate("牌局连接已断开，等待重新同步"))
            } else { None });
        }
        if direction > 2 || data.is_empty() || data.len() > MAX_FRAME {
            return Ok(Some(self.invalidate("消息不完整，请重新进入牌局")));
        }
        if self.flows.len() >= 8 && !self.flows.contains_key(&connection) {
            return Ok(Some(self.invalidate("连接已变化，请重新进入牌局")));
        }
        let parsed = self.flows.entry(connection)
            .or_insert_with(|| MajsoulBridge::new(None, None))
            .parse(if direction == 1 { Direction::Up } else { Direction::Down }, data);

        if parsed.parsed.is_none() {
            // A lost/unknown response on the active game connection means we
            // cannot prove the mirror is current. Never keep a stale discard.
            return Ok(if self.active == Some(connection) {
                Some(self.invalidate("牌局协议未能同步，请重新进入牌局"))
            } else { None });
        }
        let method = &parsed.parsed.as_ref().unwrap().method;
        if self.active == Some(connection) && direction == 1 &&
            matches!(method.as_str(), ".lq.FastTest.inputOperation" | ".lq.FastTest.inputChiPengGang") {
            self.input_pending = true;
            return Ok(Some(self.render()?));
        }
        if parsed.events.is_empty() { return Ok(None); }
        if self.active != Some(connection) && !parsed.events.iter().any(|e|
            matches!(e, MjaiEvent::StartGame { id: Some(_), .. })) {
            return Ok(None);
        }
        self.input_pending = false;
        for event in parsed.events {
            if let MjaiEvent::StartGame { id: Some(seat), num_players, .. } = &event {
                anyhow::ensure!((3..=4).contains(num_players) && seat < num_players, "invalid seat");
                self.active = Some(connection);
            }
            if self.active == Some(connection) { self.feed(&event)?; }
        }
        Ok(Some(self.render()?))
    }

    fn feed(&mut self, event: &MjaiEvent) -> anyhow::Result<()> {
        if let MjaiEvent::StartGame { id, num_players, .. } = event {
            self.round_ready = false;
            self.input_pending = false;
            self.engine = match id {
                Some(seat) if *seat < *num_players && matches!(num_players, 3 | 4) => {
                    Some(native_bot::defaults::engine(*num_players, *seat)?)
                }
                _ => None,
            };
        }
        self.tracker.handle(event)?;
        if let Some(engine) = self.engine.as_mut() {
            engine.feed_line(&serde_json::to_string(event)?);
        }
        match event {
            MjaiEvent::StartKyoku { .. } => self.round_ready = self.engine.is_some(),
            MjaiEvent::EndKyoku | MjaiEvent::EndGame { .. } => self.round_ready = false,
            _ => {},
        }
        self.revision += 1;
        Ok(())
    }

    fn render(&mut self) -> anyhow::Result<Value> {
        if !self.round_ready {
            return Ok(waiting("已连接，等待牌局开始或下一局同步"));
        }
        let begin = Instant::now();
        let snap = self.tracker.snapshot().ok_or_else(|| anyhow::anyhow!("no snapshot"))?;
        let seat = snap.our_seat.ok_or_else(|| anyhow::anyhow!("no player seat"))?;
        let info = to_player_info(&snap, seat)?;
        anyhow::ensure!(matches!(info.hand_size(), 1 | 2 | 4 | 5 | 7 | 8 | 10 | 11 | 13 | 14), "incomplete hand");
        let analysis = analysis::analyze(&info);
        let can_act = !self.input_pending && self.tracker.our_seat_can_act() == Some(true);
        let mut recommendations = Vec::new();
        if can_act {
            if let Some(engine) = self.engine.as_mut() {
                if let Some(decision) = engine.decide()? {
                    // Use action (not candidates[0]) for a riichi: only action
                    // carries the fully resolved riichi discard in upstream.
                    for (index, (action, probability)) in decision.candidates.iter().enumerate() {
                        let action = if index == 0 { &decision.action } else { action };
                        recommendations.push(action_json(action, *probability));
                    }
                }
            }
        }
        let own = &snap.players[seat as usize];
        Ok(json!({
            "status":"live", "message": if can_act { "轮到你操作" } else { "等待其他玩家" },
            "revision":self.revision, "seat":seat, "players":snap.num_players,
            "round":format!("{}{}局 · {}本场", snap.bakaze, snap.kyoku, snap.honba),
            "turn":snap.turn_count, "hand":own.tehai, "canAct":can_act,
            "analysis":analysis, "recommendations":recommendations,
            "elapsedMs":begin.elapsed().as_millis() as u64,
            "model":"Akagi native BC", "estimated":true,
        }))
    }
}

fn action_json(action: &BotAction, probability: f32) -> Value {
    let (kind, tile) = match action {
        BotAction::Dahai { pai, .. } => ("discard", pai.as_str()),
        BotAction::Reach { pai } => ("riichi", pai.as_str()),
        BotAction::Pon { pai, .. } => ("pon", pai.as_str()),
        BotAction::Chi { pai, .. } => ("chi", pai.as_str()),
        BotAction::Daiminkan { pai, .. } => ("kan", pai.as_str()),
        BotAction::Ankan { consumed } => ("ankan", consumed.first().map(String::as_str).unwrap_or("")),
        BotAction::Kakan { pai, .. } => ("kakan", pai.as_str()),
        BotAction::Hora { .. } => ("hora", ""),
        BotAction::Kyushu => ("abort", ""),
        BotAction::Kita => ("kita", "N"),
        BotAction::Pass => ("pass", ""),
    };
    json!({"kind":kind, "tile":tile, "policyProbability":probability})
}

fn demo() -> anyhow::Result<Value> {
    let mut assistant = Assistant::new();
    // A deterministic local fixture, explicitly labelled in every UI result.
    let events = [
        json!({"type":"start_game","names":["A","B","C","D"],"id":0,"num_players":4}),
        json!({"type":"start_kyoku","bakaze":"E","dora_marker":"9p","kyoku":1,
            "honba":0,"kyotaku":0,"oya":0,"scores":[25000,25000,25000,25000],
            "tehais":[["1m","2m","3m","4m","5m","6m","7m","8m","9m","1p","2p","3p","1s"],
                ["?","?","?","?","?","?","?","?","?","?","?","?","?"],
                ["?","?","?","?","?","?","?","?","?","?","?","?","?"],
                ["?","?","?","?","?","?","?","?","?","?","?","?","?"]],"num_players":4}),
        json!({"type":"tsumo","actor":0,"pai":"9s"}),
    ];
    let begin = Instant::now();
    for event in events { assistant.feed(&serde_json::from_value(event)?)?; }
    let mut result = assistant.render()?;
    result["status"] = json!("demo");
    result["message"] = json!("本地自检样例 · 非当前牌局");
    result["loadAndInferenceMs"] = json!(begin.elapsed().as_millis() as u64);
    Ok(result)
}

fn output(value: Value) -> *mut c_char {
    CString::new(value.to_string()).expect("JSON contains no literal NUL").into_raw()
}

#[no_mangle]
pub extern "C" fn majmax_ai_reset() {
    if let Ok(mut host) = HOST.get_or_init(|| Mutex::new(Assistant::new())).lock() {
        *host = Assistant::new();
    }
}

/// Called only by the manager's background worker. Nothing here runs in the game process.
#[no_mangle]
pub unsafe extern "C" fn majmax_ai_frame(connection: u64, direction: u8, data: *const u8, len: usize) -> *mut c_char {
    if len > MAX_FRAME || (len > 0 && data.is_null()) {
        return output(waiting("消息大小异常，等待重新同步"));
    }
    let mut host = match HOST.get_or_init(|| Mutex::new(Assistant::new())).lock() {
        Ok(host) => host,
        Err(_) => return output(waiting("本地引擎异常，请重启助手")),
    };
    let input = if len == 0 { &[] } else { slice::from_raw_parts(data, len) };
    // Catch while holding the guard, so a malformed upstream event cannot
    // poison the mutex or unwind through JNI into Android's runtime.
    match catch_unwind(AssertUnwindSafe(|| host.frame(connection, direction, input))) {
        Ok(Ok(Some(value))) => output(value),
        Ok(Ok(None)) => std::ptr::null_mut(),
        _ => output(host.invalidate("本地分析未能同步，请重新进入牌局")),
    }
}

#[no_mangle]
pub extern "C" fn majmax_ai_self_test() -> *mut c_char {
    match catch_unwind(AssertUnwindSafe(demo)) {
        Ok(Ok(value)) => output(value),
        _ => output(json!({"status":"error","message":"本地模型自检失败"})),
    }
}

#[no_mangle]
pub unsafe extern "C" fn majmax_ai_free(value: *mut c_char) {
    if !value.is_null() { drop(CString::from_raw(value)); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_model_produces_legal_local_recommendation() {
        let result = demo().unwrap();
        assert_eq!(result["status"], "demo");
        assert!(result["recommendations"].as_array().unwrap().len() > 0);
        let first = &result["recommendations"][0];
        if first["kind"] == "discard" || first["kind"] == "riichi" {
            assert!(result["hand"].as_array().unwrap().contains(&first["tile"]));
        }
    }

    #[test]
    fn missed_auth_never_invents_a_hand() {
        let mut host = Assistant::new();
        assert!(host.frame(1, 0, &[1, 2, 3]).unwrap().is_none());
        assert!(!host.round_ready);
        assert_eq!(host.render().unwrap()["status"], "waiting");
    }

    #[test]
    fn active_disconnect_clears_recommendations() {
        let mut host = Assistant::new();
        host.active = Some(10);
        host.round_ready = true;
        let result = host.frame(10, 2, &[]).unwrap().unwrap();
        assert_eq!(result["status"], "waiting");
        assert!(host.engine.is_none());
        assert!(host.active.is_none());
    }
}
