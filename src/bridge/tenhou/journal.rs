//! What a capture run has seen of the hand in progress, kept across the
//! WebSocket flows a reconnect opens.
//!
//! A rejoining client opens a fresh socket and so gets a fresh bridge, which
//! sees only the `<REINIT/>` snapshot. The flow that dropped had watched the
//! hand up to the moment it dropped. The journal hands that record on, so
//! the rebuild only has to work out what happened while no flow was
//! watching (see [`super::rebuild`]).
//!
//! Every Tenhou bridge of one capture run shares one slot. A bridge records
//! what it emits: `start_kyoku` opens a new record, the hand's end (or a
//! game boundary) closes it, everything between is appended. Events a
//! suspended flow withholds are not emitted and so not recorded.

use crate::schema::MjaiEvent;
use std::sync::{Arc, Mutex};

/// The events emitted for one hand, `start_kyoku` first.
#[derive(Debug, Clone)]
pub struct Journal {
    /// Our seat on the flow that emitted them.
    pub seat: u8,
    pub num_players: u8,
    pub events: Vec<MjaiEvent>,
}

/// The slot the bridges of one capture run share.
pub type SharedJournal = Arc<Mutex<Option<Journal>>>;

pub fn new_shared() -> SharedJournal {
    Arc::default()
}

/// Append what a flow seated at `seat` just emitted.
pub fn record(slot: &SharedJournal, seat: u8, num_players: u8, events: &[MjaiEvent]) {
    let Ok(mut journal) = slot.lock() else {
        return;
    };
    for event in events {
        match event {
            MjaiEvent::StartKyoku { .. } => {
                *journal = Some(Journal {
                    seat,
                    num_players,
                    events: vec![event.clone()],
                });
            }
            MjaiEvent::StartGame { .. }
            | MjaiEvent::Hora { .. }
            | MjaiEvent::Ryukyoku { .. }
            | MjaiEvent::EndKyoku
            | MjaiEvent::EndGame { .. } => *journal = None,
            _ => {
                if let Some(j) = journal.as_mut().filter(|j| j.seat == seat) {
                    j.events.push(event.clone());
                }
            }
        }
    }
}

/// The recorded events of the hand a snapshot shows, if the slot holds
/// that hand: same seat and table size, same round, honba and dealer.
pub fn of_hand(
    slot: &SharedJournal,
    seat: u8,
    num_players: u8,
    bakaze: &str,
    kyoku: u8,
    honba: u8,
    oya: u8,
) -> Option<Vec<MjaiEvent>> {
    let journal = slot.lock().ok()?;
    let j = journal.as_ref()?;
    let same = j.seat == seat
        && j.num_players == num_players
        && matches!(
            j.events.first(),
            Some(MjaiEvent::StartKyoku { bakaze: b, kyoku: k, honba: h, oya: o, .. })
                if b == bakaze && *k == kyoku && *h == honba && *o == oya
        );
    same.then(|| j.events.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start(kyoku: u8) -> MjaiEvent {
        MjaiEvent::StartKyoku {
            bakaze: "E".into(),
            dora_marker: "1m".into(),
            kyoku,
            honba: 0,
            kyotaku: 0,
            oya: kyoku - 1,
            scores: vec![25_000; 4],
            tehais: vec![vec!["?".to_string(); 13]; 4],
            num_players: 4,
            catch_up: 0,
        }
    }

    fn tsumo(actor: u8) -> MjaiEvent {
        MjaiEvent::Tsumo {
            actor,
            pai: "?".into(),
        }
    }

    #[test]
    fn a_hand_is_recorded_from_its_start_until_its_end() {
        let slot = new_shared();
        record(&slot, 2, 4, &[tsumo(0)]);
        assert!(of_hand(&slot, 2, 4, "E", 1, 0, 0).is_none(), "no hand open");

        record(&slot, 2, 4, &[start(1), tsumo(0)]);
        record(&slot, 2, 4, &[tsumo(1)]);
        let events = of_hand(&slot, 2, 4, "E", 1, 0, 0).expect("the hand in progress");
        assert_eq!(events.len(), 3);

        assert!(of_hand(&slot, 1, 4, "E", 1, 0, 0).is_none(), "another seat");
        assert!(of_hand(&slot, 2, 4, "E", 2, 0, 1).is_none(), "another hand");
        assert!(
            of_hand(&slot, 2, 3, "E", 1, 0, 0).is_none(),
            "another table"
        );

        record(&slot, 2, 4, &[MjaiEvent::EndKyoku]);
        assert!(
            of_hand(&slot, 2, 4, "E", 1, 0, 0).is_none(),
            "the hand ended"
        );
    }

    /// A rejoined flow's own rebuilt hand replaces the record, and is the
    /// record a second rejoin in the same hand carries on from.
    #[test]
    fn the_next_start_kyoku_replaces_the_record() {
        let slot = new_shared();
        record(&slot, 0, 4, &[start(1), tsumo(0), tsumo(1)]);
        record(&slot, 0, 4, &[start(1), tsumo(0)]);
        assert_eq!(of_hand(&slot, 0, 4, "E", 1, 0, 0).unwrap().len(), 2);
    }
}
