//! Server-granted time budget for the current decision window.
//!
//! Majsoul attaches an `OptionalOperationList` to every action that opens a
//! decision window for our seat; its `time_fixed` / `time_add` fields (both
//! **milliseconds** on the wire — unlike `GameDetailRule`, whose same-named
//! fields are seconds) are the base thinking time and the extra time pool
//! granted for that window.
//!
//! The Majsoul bridge is the single writer: on every `ActionPrototype` it
//! either stores the freshly-opened window's budget (operation present and
//! addressed to our seat) or clears the slot (no operation — no window is
//! open). The autoplay manager is the reader. The value is a per-window
//! snapshot straight from the server, never locally accounted, so it cannot
//! drift out of sync across reconnects, spectating or manual takeover.
//!
//! The slot is a `std::sync::RwLock` because the writer runs inside the
//! bridge's synchronous `parse()` path (already under a `std::sync::Mutex`)
//! and the reader only takes a copy; the critical section is a pointer-sized
//! write and is never held across an await.

use std::sync::{Arc, RwLock};
use std::time::Instant;

/// Which action type carried the operation list. Debug/telemetry only —
/// the delay model keys off the mjai action, not off this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetSource {
    /// `ActionNewRound` — the opening hand (dealer's 14-tile window).
    NewRound,
    /// `ActionDealTile` — our own draw.
    DealTile,
    /// `ActionDiscardTile` — a claim window on another seat's discard.
    DiscardTile,
    /// `ActionChiPengGang` — post-call windows (e.g. discard after a call).
    ChiPengGang,
    /// `ActionAnGangAddGang` — chankan windows (ron on a kakan; kokushi
    /// robbing an ankan).
    AnGangAddGang,
    /// `ActionBaBei` (3p) — 胡拔北 windows.
    BaBei,
}

/// Time budget the server granted for the *current* decision window.
#[derive(Debug, Clone, Copy)]
pub struct TimeBudget {
    /// Base thinking time for this window (`operation.time_fixed`), ms.
    pub fixed_ms: u32,
    /// Extra time pool (`operation.time_add`), ms. Whether this is the
    /// remaining bank or a per-window grant is still unverified — treat
    /// it as "may be consumed" and spend it conservatively.
    pub add_ms: u32,
    /// When we decoded the frame that opened the window. Network + proxy
    /// latency is already inside the frame arrival time, so this is the
    /// closest observable point to "server started the clock".
    pub opened_at: Instant,
    /// Which action carried the operation list.
    pub source: BudgetSource,
}

impl TimeBudget {
    /// Milliseconds elapsed since the window opened, saturating.
    pub fn elapsed_ms(&self) -> u32 {
        u32::try_from(self.opened_at.elapsed().as_millis()).unwrap_or(u32::MAX)
    }
}

/// Shared slot: bridge writes, autoplay manager reads.
pub type SharedTimeBudget = Arc<RwLock<Option<TimeBudget>>>;

/// Fresh empty slot.
pub fn new_shared() -> SharedTimeBudget {
    Arc::new(RwLock::new(None))
}
