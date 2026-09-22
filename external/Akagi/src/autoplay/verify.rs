//! Did the click actually land?
//!
//! A synthetic click can be swallowed — the UI was still animating, the
//! button had not finished popping in, the engine sampled the cursor a
//! frame too early. The press itself reports success either way: the page
//! dispatched the events, and nothing downstream says whether the client
//! did anything with them.
//!
//! The client's own uplink is the answer. Accepting a discard, a call or a
//! win makes it send `.lq.FastTest.inputOperation` /
//! `.lq.FastTest.inputChiPengGang` — so a click followed by one of those is
//! a click that registered, and a click followed by silence is one to try
//! again. The bridge counts them here as it parses; the autoplay manager
//! takes a ticket before pressing and asks afterwards whether the count
//! moved.
//!
//! A counter rather than a timestamp: the question is "did *another* one
//! happen since my ticket", which a monotonic count answers without any
//! clock comparison, and which cannot be confused by two inputs landing in
//! the same millisecond.

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

/// What an input command was, to the extent autoplay needs to tell them
/// apart.
///
/// Riichi is distinguished because only riichi has a failure that is
/// worse than nothing happening: the declaration and the discard are two
/// presses, and if the first is lost the second still discards — the tile
/// the bot picked as its riichi tile goes out with no riichi behind it.
/// On the wire the two are unmistakable (`inputOperation` `type: 7`
/// versus `type: 1`).
///
/// A plain discard is distinguished because it is the one input the
/// client also produces *on its own* in a way the client-initiated filter
/// cannot catch: an own-turn window that runs out is answered with a
/// tsumogiri stamped with a plausible `timeuse` and no `auto_operation`
/// flag — indistinguishable on the wire from a pressed tile. A plan that
/// clicked only action buttons can never legitimately produce a `type: 1`
/// discard, so counting one as proof the button landed would report
/// retries as "registered" at the exact moments the presses had
/// demonstrably failed (and reset the dead-click counter that decides
/// recovery reloads).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    /// A riichi declaration (and its discard).
    Reach,
    /// A plain discard (`inputOperation` `type: 1`).
    Discard,
    /// Anything else the client accepted.
    Other,
}

/// Counts input commands the client has sent, by kind.
#[derive(Debug, Default)]
pub struct InputWatch {
    sent: AtomicU64,
    reach: AtomicU64,
    non_discard: AtomicU64,
}

/// A snapshot of the counts, taken before a click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputTicket {
    sent: u64,
    reach: u64,
    non_discard: u64,
}

impl InputWatch {
    /// The bridge saw the client send an input command.
    pub fn note_sent(&self, kind: InputKind) {
        self.sent.fetch_add(1, Ordering::Relaxed);
        if kind == InputKind::Reach {
            self.reach.fetch_add(1, Ordering::Relaxed);
        }
        if kind != InputKind::Discard {
            self.non_discard.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Snapshot to compare against later.
    pub fn ticket(&self) -> InputTicket {
        InputTicket {
            sent: self.sent.load(Ordering::Relaxed),
            reach: self.reach.load(Ordering::Relaxed),
            non_discard: self.non_discard.load(Ordering::Relaxed),
        }
    }

    /// Whether any input command has been sent since `ticket` was taken.
    pub fn sent_since(&self, ticket: InputTicket) -> bool {
        self.sent.load(Ordering::Relaxed) != ticket.sent
    }

    /// Whether an input command *other than a plain discard* has been sent
    /// since `ticket` was taken — the proof standard for a plan whose
    /// clicks were all action buttons, where a discard can only mean the
    /// client's own turn-timeout tsumogiri.
    pub fn non_discard_since(&self, ticket: InputTicket) -> bool {
        self.non_discard.load(Ordering::Relaxed) != ticket.non_discard
    }

    /// Whether a *riichi* input has been sent since `ticket` was taken.
    pub fn reach_since(&self, ticket: InputTicket) -> bool {
        self.reach.load(Ordering::Relaxed) != ticket.reach
    }
}

/// Shared between the bridge (which counts) and the autoplay manager
/// (which checks). `Relaxed` ordering throughout: the only thing that
/// matters is that the count eventually differs, and every observation is
/// separated from the write by a real timer wait.
pub type SharedInputWatch = Arc<InputWatch>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ticket_only_moves_when_input_is_sent() {
        let w = InputWatch::default();
        let t = w.ticket();
        assert!(!w.sent_since(t), "nothing sent yet");
        w.note_sent(InputKind::Other);
        assert!(w.sent_since(t));
    }

    /// The case this exists for: a riichi plan whose declaration press was
    /// lost still produces an input command — the discard — so presence
    /// alone reads as success while the hand has in fact discarded the
    /// riichi tile without declaring.
    #[test]
    fn a_plain_discard_does_not_pass_for_a_riichi() {
        let w = InputWatch::default();
        let t = w.ticket();
        w.note_sent(InputKind::Other);
        assert!(w.sent_since(t), "something was sent");
        assert!(!w.reach_since(t), "but it was not the riichi");

        let t = w.ticket();
        w.note_sent(InputKind::Reach);
        assert!(w.reach_since(t));
    }

    /// Each press takes its own ticket: input from the *previous* decision
    /// must not make the next click look like it registered.
    #[test]
    fn each_ticket_is_independent() {
        let w = InputWatch::default();
        w.note_sent(InputKind::Other);
        let t = w.ticket();
        assert!(!w.sent_since(t), "a fresh ticket starts clean");
        w.note_sent(InputKind::Other);
        assert!(w.sent_since(t));
    }

    /// An own-turn window that expires is answered by the client with a
    /// tsumogiri that looks exactly like a pressed tile on the wire (small
    /// `timeuse`, no `auto_operation`). It must not stand in as proof that
    /// an action-button press landed — a plan that clicked only buttons
    /// cannot legitimately produce a plain discard.
    #[test]
    fn a_plain_discard_does_not_prove_a_button_press() {
        let w = InputWatch::default();
        let t = w.ticket();
        w.note_sent(InputKind::Discard);
        assert!(w.sent_since(t), "a discard is still an input");
        assert!(
            !w.non_discard_since(t),
            "but it proves nothing about a button"
        );

        // Anything that is not a plain discard does prove it.
        let t = w.ticket();
        w.note_sent(InputKind::Other);
        assert!(w.non_discard_since(t));
        let t = w.ticket();
        w.note_sent(InputKind::Reach);
        assert!(w.non_discard_since(t), "a riichi declaration counts");
    }

    /// Two inputs inside the same millisecond are two inputs — the reason
    /// this is a count and not a timestamp.
    #[test]
    fn back_to_back_inputs_both_count() {
        let w = InputWatch::default();
        let t0 = w.ticket();
        w.note_sent(InputKind::Other);
        let t1 = w.ticket();
        w.note_sent(InputKind::Other);
        assert!(w.sent_since(t0) && w.sent_since(t1));
    }
}
