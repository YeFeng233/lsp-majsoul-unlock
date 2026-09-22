# `src/game_state` — Live game-state tracker

Wraps [`riichienv-core`](https://crates.io/crates/riichienv-core) to
maintain an authoritative, queryable mirror of the current mahjong game
fed by the MJAI event stream from the proxy bridge.

## Why a wrapper?

`riichienv-core` is great as a simulation engine but its public API is
shaped for RL training: `Vec<...>` everywhere, raw `u8` tile IDs in
0..136 with the red-five convention (16/52/88), `Phase` as a bare enum,
and so on. None of that is what a UI or downstream IPC layer wants.

This module:

1. Translates Akagi's own `schema::MjaiEvent` into the riichienv flavor
   (single field-level mismatch on `StartGame.id`, otherwise direct
   JSON round-trip — see `convert.rs`).
2. Drives **either** `GameState::apply_mjai_event` (4p) **or**
   `GameState3P::apply_mjai_event` (3p sanma) with the converted events.
   Both engines accept the same `riichienv_core::replay::MjaiEvent` enum
   (which already includes the `Kita` variant), so the dispatch surface
   is just a `match self.state` against `enum TrackedGame { Four, Three }`.
3. Provides a `GameStateSnapshot` whose tiles are mjai strings and
   whose enums use snake-case discriminants — straight to the wire.
   `players` is a `Vec<PlayerSnapshot>` of length `num_players`;
   `PlayerSnapshot.kita_tiles` carries the 3p kita pool (empty in 4p).
4. Wraps the score / hand-evaluator helpers behind a stable interface
   so a riichienv API bump only touches this module. `calculate_score`
   takes `num_players` so 3p tsumo splits and honba math come out
   right.

## Files

| File          | Purpose                                              |
|---------------|------------------------------------------------------|
| `convert.rs`  | `to_riichienv(&AkagiEvent) -> Result<Option<RiEvent>>` |
| `tracker.rs`  | `GameTracker`, `spawn(rx) -> Arc<Mutex<GameTracker>>`, `our_seat_can_act()` |
| `snapshot.rs` | `GameStateSnapshot`, `PlayerSnapshot`, `MeldSnapshot` |
| `score.rs`    | `calculate_score`, `waits_for`, `is_tenpai`          |

## Wiring

Spawned from `lib.rs` once the MJAI bus exists:

```rust
let tracker = game_state::spawn(mjai_bus.subscribe());
// tracker: Arc<Mutex<GameTracker>>
//   → IPC commands (future) read snapshot via `tracker.lock().await.snapshot()`
```

The handle is held by `AppState` so future IPC commands can pull
snapshots without keeping a separate reference.

## Querying

```rust
let snap = tracker.lock().await.snapshot().expect("game in progress");
println!("oya: {}, dora: {:?}", snap.oya, snap.dora_markers);
```

`snapshot()` returns `None` until the first `start_game` event arrives.

## The post-tracker bus, and `can_act`

`spawn_with_post` re-emits every event on a second bus *after* the engine
has applied it, as an `event_bus::TrackedEvent` — the event plus what the
engine then had to say about our seat:

```rust
pub struct TrackedEvent {
    pub event: MjaiEvent,
    /// Is the engine offering our seat a choice in the state this event
    /// produced? `None` when it has no opinion (no game, no seat).
    pub can_act: Option<bool>,
}
```

Subscribers to that bus (the analysis runner, the bot manager) can rely on
the mirror being current when it fires, which the raw `MjaiBus` cannot
promise.

`can_act` rides *with* the event rather than being looked up on arrival, and
that is the whole reason the type exists. One server frame can carry several
seats' actions; the tracker applies them all in microseconds while a
subscriber that pauses between events — the bot manager waits on inference —
is still holding the first. Asking the tracker at that point answers about a
state several events too late. `our_seat_can_act()` is therefore read under
the same lock that applied the event.

What counts as "can act" is: a legal-action set that is neither empty nor
exactly `[Pass]`. riichienv hands a `Pass` to every seat while it is in its
response phase — including seats with nothing to claim — so a lone pass is
the engine saying "not yours", not offering a decline.

## Score / wait helpers

These are pure functions; no state required:

```rust
use akagi::game_state::{calculate_score, waits_for};

// 3 han 30 fu, non-dealer ron, 0 honba, 4p.
let s = calculate_score(3, 30, false, false, 0, 4);
assert_eq!(s.total, 3_900);

let waits = waits_for("123456789m123p1s")?;
assert_eq!(waits, vec!["1s"]);
```

The hand string is `riichienv`'s MPSZ notation, not mjai. Use
`(p123m)` etc. for melds.

## Adding a new event handler

`riichienv` already handles every protocol event in
`apply_mjai_event`. The only thing you'd add here is:

- A new field in `GameStateSnapshot` if the engine exposes something
  the UI needs but we're not surfacing yet (e.g. `last_win_results`).
- A patch in `convert.rs` if a new mjai event variant has a shape
  mismatch between Akagi and riichienv.

## Future: GameStateBus

When the IPC layer needs live state push (toast-style or reactive UI),
add a `GameStateBus` to `crate::event_bus` and have `tracker::run`
publish a snapshot on every event (or every N events). The current
design intentionally stops one step short of that — snapshot-on-pull
is enough for the read-only commands we need first.
