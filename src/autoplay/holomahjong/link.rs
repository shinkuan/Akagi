//! What HoloMahjong's bridge, planner and h2c relay share.
//!
//! The server announces each decision it is waiting on — a draw of ours, or
//! someone's discard we may claim — together with the exact calls it will
//! accept and the time it allows. The bridge records that as the open
//! [`Window`]; the planner builds the client's own hub call from it; the
//! relay that carries the live hub stream transmits the call (see
//! `proxy::h2c::Splice`). Every tapped h2c connection subscribes, and only
//! the one that most recently opened the hub stream writes: it names itself
//! in [`HoloLink::owner`].

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::broadcast;

use crate::schema::mjai::Tile;

pub type SharedHoloLink = Arc<HoloLink>;

/// The gRPC path of the battle connection's hub stream.
pub const HUB_PATH: &str = "/IMahjongHub/Connect";

pub struct HoloLink {
    /// Hub messages (MessagePack, unframed) for the live hub stream.
    tx: broadcast::Sender<Vec<u8>>,
    /// Relay id allowed to write; 0 when none holds a hub stream.
    owner: Arc<AtomicU64>,
    next_relay: AtomicU64,
    table: Mutex<Table>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Table {
    /// Our seat on the wire (not the mjai actor). Every call that names a
    /// seat names this one.
    pub seat: Option<u8>,
    pub window: Option<Window>,
}

/// One decision the server is waiting on from us.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    /// When the bridge read the event that opened it; also its identity.
    pub opened_at: Instant,
    pub kind: WindowKind,
    /// Calls on offer, in the server's notation — sent back verbatim.
    pub chi: Vec<String>,
    pub pon: Vec<String>,
    /// Claim: open kans. Turn: closed and added kans.
    pub kan: Vec<String>,
    /// Kita on offer (`"z4"`, or `"z4_"` for the tile just drawn).
    pub kita: Vec<String>,
    /// The server would score a win: tsumo on a turn, ron on a claim.
    pub can_win: bool,
    /// Base time for this decision, ms.
    pub base_ms: u32,
    /// Time bank left, ms; spent once the base runs out.
    pub bank_ms: u32,
    /// Everything the server allows before it acts for us, ms.
    pub total_ms: u32,
    /// The client already sent its own answer to this window.
    pub answered: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowKind {
    /// Our turn to discard; `drawn` is the tile just drawn (none after a call).
    Turn { drawn: Option<Tile> },
    /// Another seat's tile we may claim.
    Claim,
}

impl Default for HoloLink {
    fn default() -> Self {
        Self::new()
    }
}

impl HoloLink {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(16);
        Self {
            tx,
            owner: Arc::new(AtomicU64::new(0)),
            next_relay: AtomicU64::new(1),
            table: Mutex::new(Table::default()),
        }
    }

    pub fn table(&self) -> Table {
        self.table.lock().expect("holo table poisoned").clone()
    }

    pub fn set_seat(&self, seat: Option<u8>) {
        self.table.lock().expect("holo table poisoned").seat = seat;
    }

    pub fn open(&self, window: Window) {
        self.table.lock().expect("holo table poisoned").window = Some(window);
    }

    pub fn close(&self) {
        self.table.lock().expect("holo table poisoned").window = None;
    }

    /// The client sent a decision of its own.
    pub fn mark_answered(&self) {
        if let Some(w) = &mut self.table.lock().expect("holo table poisoned").window {
            w.answered = true;
        }
    }

    /// Identity of the open window, if any.
    pub fn window_id(&self) -> Option<Instant> {
        self.table
            .lock()
            .expect("holo table poisoned")
            .window
            .as_ref()
            .map(|w| w.opened_at)
    }

    /// Whether `id` is still the open window and nobody has answered it.
    pub fn awaits(&self, id: Instant) -> bool {
        self.table
            .lock()
            .expect("holo table poisoned")
            .window
            .as_ref()
            .is_some_and(|w| w.opened_at == id && !w.answered)
    }

    /// Queue a hub message for the live hub stream. `false` when no relay
    /// holds one (capture not running, or no game connection).
    pub fn send(&self, message: Vec<u8>) -> bool {
        self.owner.load(Ordering::Acquire) != 0 && self.tx.send(message).is_ok()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Vec<u8>> {
        self.tx.subscribe()
    }

    /// A fresh id for a relay.
    pub fn relay_id(&self) -> u64 {
        self.next_relay.fetch_add(1, Ordering::Relaxed)
    }

    /// Shared with the relays, which claim and release it themselves.
    pub fn owner(&self) -> Arc<AtomicU64> {
        self.owner.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> Window {
        Window {
            opened_at: Instant::now(),
            kind: WindowKind::Claim,
            chi: Vec::new(),
            pon: vec!["p555=".into()],
            kan: Vec::new(),
            kita: Vec::new(),
            can_win: false,
            base_ms: 5000,
            bank_ms: 20000,
            total_ms: 25000,
            answered: false,
        }
    }

    #[test]
    fn a_window_awaits_until_answered_or_replaced() {
        let link = HoloLink::new();
        let w = window();
        let id = w.opened_at;
        link.open(w);
        assert!(link.awaits(id));
        link.mark_answered();
        assert!(!link.awaits(id), "the client answered it itself");

        let mut next = window();
        next.opened_at = id + std::time::Duration::from_millis(1);
        link.open(next);
        assert!(!link.awaits(id), "a newer window replaced it");
        link.close();
        assert_eq!(link.window_id(), None);
    }

    #[tokio::test]
    async fn messages_go_out_only_while_a_relay_holds_the_hub() {
        let link = HoloLink::new();
        let mut rx = link.subscribe();
        assert!(!link.send(vec![1]), "no relay holds a hub stream");
        let id = link.relay_id();
        assert_ne!(id, 0, "0 means nobody");
        link.owner().store(id, Ordering::Release);
        assert!(link.send(vec![2]));
        assert_eq!(rx.recv().await.unwrap(), vec![2]);
        link.owner().store(0, Ordering::Release);
        assert!(!link.send(vec![3]));
    }
}
