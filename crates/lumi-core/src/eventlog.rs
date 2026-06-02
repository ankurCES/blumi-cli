//! Sequenced, replayable event fan-out.
//!
//! Every event a session produces flows through here: it's assigned a
//! monotonic `seq`, appended to a bounded ring buffer, and broadcast to all
//! subscribers. The ring buffer + `since()` give late or reconnecting
//! subscribers (e.g. an SSE client with `Last-Event-ID`) lossless replay,
//! which is why slow broadcast receivers can drop frames safely.

use lumi_protocol::{Envelope, Event, SessionId};
use std::collections::VecDeque;
use tokio::sync::broadcast;

/// Default number of recent events retained for replay.
const DEFAULT_RING_CAP: usize = 1024;
/// Broadcast channel capacity (per-subscriber lag buffer).
const BROADCAST_CAP: usize = 4096;

/// The sequenced event stream for one session.
pub struct EventLog {
    session: SessionId,
    next_seq: u64,
    ring: VecDeque<Envelope>,
    cap: usize,
    tx: broadcast::Sender<Envelope>,
}

impl EventLog {
    pub fn new(session: SessionId) -> Self {
        Self::with_capacity(session, DEFAULT_RING_CAP)
    }

    pub fn with_capacity(session: SessionId, cap: usize) -> Self {
        let (tx, _rx) = broadcast::channel(BROADCAST_CAP);
        EventLog {
            session,
            next_seq: 1,
            ring: VecDeque::with_capacity(cap.min(256)),
            cap,
            tx,
        }
    }

    /// Assign a sequence number, retain for replay, and broadcast. Returns the
    /// sequenced envelope.
    pub fn publish(&mut self, event: Event) -> Envelope {
        let env = Envelope {
            seq: self.next_seq,
            session: self.session.clone(),
            event,
        };
        self.next_seq += 1;
        self.ring.push_back(env.clone());
        while self.ring.len() > self.cap {
            self.ring.pop_front();
        }
        // Ignore send errors: zero subscribers is fine.
        let _ = self.tx.send(env.clone());
        env
    }

    /// Subscribe to live events. Combine with [`since`](Self::since) for a
    /// gap-free attach: read the backlog first, then drain this receiver.
    pub fn subscribe(&self) -> broadcast::Receiver<Envelope> {
        self.tx.subscribe()
    }

    /// All retained events with `seq > after_seq` (use `0` for "everything
    /// still buffered").
    pub fn since(&self, after_seq: u64) -> Vec<Envelope> {
        self.ring
            .iter()
            .filter(|e| e.seq > after_seq)
            .cloned()
            .collect()
    }

    /// The seq that the next published event will get.
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumi_protocol::DoneReason;

    fn ev(n: u32) -> Event {
        Event::Token {
            text: n.to_string(),
        }
    }

    #[test]
    fn assigns_monotonic_seq() {
        let mut log = EventLog::new(SessionId::from("s"));
        assert_eq!(log.publish(ev(1)).seq, 1);
        assert_eq!(log.publish(ev(2)).seq, 2);
        assert_eq!(log.next_seq(), 3);
    }

    #[test]
    fn since_returns_tail() {
        let mut log = EventLog::new(SessionId::from("s"));
        for i in 0..5 {
            log.publish(ev(i));
        }
        let tail = log.since(3);
        assert_eq!(tail.len(), 2);
        assert_eq!(tail[0].seq, 4);
    }

    #[test]
    fn ring_is_bounded() {
        let mut log = EventLog::with_capacity(SessionId::from("s"), 3);
        for i in 0..10 {
            log.publish(ev(i));
        }
        let all = log.since(0);
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].seq, 8); // oldest retained
        assert_eq!(all[2].seq, 10);
    }

    #[tokio::test]
    async fn subscribers_receive_live_events() {
        let mut log = EventLog::new(SessionId::from("s"));
        let mut rx = log.subscribe();
        log.publish(Event::TurnDone {
            reason: DoneReason::Completed,
        });
        let got = rx.recv().await.unwrap();
        assert_eq!(got.seq, 1);
        assert!(matches!(got.event, Event::TurnDone { .. }));
    }
}
