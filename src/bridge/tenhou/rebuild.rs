//! Rebuilding the hand in progress from a `<REINIT/>` snapshot.
//!
//! A rejoining client gets `<REINIT/>` instead of `<INIT/>`: the hand in
//! progress as a *snapshot* — every seat's river in order (called tiles
//! taken out, `255` before the riichi tile), every seat's calls in order, our
//! concealed hand, the scores, the sticks and the dora indicators. What it
//! does not say is how the seats' actions interleave: when each call
//! happened against the other seats' discards. An mjai consumer needs a log,
//! so this module looks for a history that produces exactly this snapshot
//! and emits it as mjai events.
//!
//! Usually most of that history is known: Akagi was watching the hand on
//! the flow that dropped, and the capture run kept what it emitted (see
//! [`super::journal`]). Given that record, the history must begin with it,
//! word for word, and only the stretch no flow saw — typically a handful of
//! events — is inferred as below. Without one (Akagi started mid-hand, or
//! the record does not fit the snapshot) the whole hand is inferred.
//!
//! ## What pins the history down
//!
//! Turn order does most of the work. A discard nobody calls passes the turn
//! to the next seat; a call moves it to the caller, who discards next (after
//! a replacement draw for an open kan). So:
//!
//! - With no calls the history is the plain rotation from the dealer, and it
//!   is exact.
//! - A chi or pon on the discard of the seat to the caller's left skips
//!   nobody, so the order across seats stays exact. Only where the called
//!   tile sat in its discarder's river is open.
//! - A pon or open kan on a discard from across the table or from the right
//!   skips one or two seats. When it happened decides how the skipped seats'
//!   later discards line up with everyone else's, and that can have more
//!   than one answer.
//! - Closed kans, added kans and kita happen inside the seat's own turn and
//!   move nobody else.
//!
//! The search walks the turns in order. At each discard it either takes the
//! discarder's next river tile or a tile a caller took from it, preferring
//! the river tile, so a call whose time is open lands as late as the
//! snapshot allows (kans likewise). Besides turn order it holds to three
//! rules: a seat in riichi makes no call; the discard after a chi or pon is
//! never a swap-call (kuikae) tile; and while we are in riichi no other seat
//! discards a tile we wait on — we would have won on it. The last rule does
//! not apply when we are furiten, and is dropped if no history satisfies it.
//!
//! A history can stop on our turn (holding a draw or a fresh call), after
//! another seat's chi or pon, with another seat holding a replacement draw
//! (its kan or kita shows, the draw does not), after a discard, or before
//! the dealer's first draw. Another seat's ordinary draw leaves no trace in
//! the snapshot, so a snapshot taken after one ends before it.
//!
//! ## What is made up
//!
//! - Our own draws. Every turn draws the tile it then discards (or kans, or
//!   sets aside as kita), and the opening hand is whatever makes that end at
//!   the hand we hold now. Every tile in it is one we really held; only the
//!   moments it arrived are invented.
//! - Other seats' tsumogiri flags. A discard right after a call is a tedashi
//!   and every discard after a seat's riichi is a tsumogiri; the rest are
//!   unknown and reported as tedashi.
//! - The timing of calls and kans, where the snapshot leaves it open.

use super::meld::{Meld, MeldKind};
use super::tile::tenhou_to_mjai_one;
use crate::schema::MjaiEvent;
use anyhow::{bail, ensure, Result};
use std::collections::HashSet;

/// How many turn positions one search may visit before giving up. A real
/// hand needs a few hundred at most; the cap only bounds a snapshot that no
/// history fits.
const VISIT_LIMIT: usize = 200_000;

/// The `<REINIT/>` snapshot, translated to mjai-absolute seats. Tiles are
/// Tenhou indices (`0..=135`), so every tile is one physical copy.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// 3 or 4.
    pub num_players: u8,
    /// Our seat.
    pub seat: u8,
    pub oya: u8,
    pub bakaze: String,
    /// 1..=4
    pub kyoku: u8,
    pub honba: u8,
    /// Riichi sticks on the table now, this hand's deposits included.
    pub kyotaku: u8,
    /// Scores now, in points, this hand's riichi deposits already paid.
    pub scores: Vec<i32>,
    /// Dora indicators revealed so far, the first one first.
    pub dora_markers: Vec<u32>,
    /// Our concealed tiles.
    pub hand: Vec<u32>,
    /// Indexed by seat, `num_players` long.
    pub seats: Vec<SeatSnapshot>,
}

/// One seat's part of the snapshot.
#[derive(Debug, Clone, Default)]
pub struct SeatSnapshot {
    /// Discards still in the river, oldest first. Called tiles are not here.
    pub river: Vec<u32>,
    /// How many river tiles precede the riichi mark, if the seat declared.
    pub riichi_at: Option<usize>,
    /// Calls and kita, in the order the frame lists them.
    pub melds: Vec<SnapMeld>,
}

/// One entry of a seat's `m<n>` list.
#[derive(Debug, Clone)]
pub enum SnapMeld {
    Call(Meld),
    /// Nukidora, with the north tile set aside.
    Kita(u32),
}

/// Where live play picks up after the rebuilt history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// This seat draws next.
    Draw(u8),
    /// This seat has drawn or called and owes a discard.
    Discard(u8),
}

/// A history that fits the snapshot.
#[derive(Debug, Clone)]
pub struct Rebuilt {
    /// `start_kyoku`, then the hand up to now.
    pub events: Vec<MjaiEvent>,
    pub next: Next,
    /// True when the hand had calls or kans whose timing had to be chosen
    /// (outside what was recorded, when a record was given).
    pub approximate: bool,
    /// How many events after `start_kyoku` came from the record of the
    /// hand rather than being inferred. 0 without a usable record.
    pub recorded: usize,
    /// The most recent discard: seat and tile.
    pub last_discard: Option<(u8, String)>,
    /// The tile we hold as this turn's draw, when the history ends on our
    /// draw (`None` after our chi/pon, or on another seat's turn). The frame
    /// lists our hand sorted, so outside riichi which tile it is cannot be
    /// known; the bridge makes its own hand agree with this one.
    pub drawn: Option<u32>,
}

/// Find a history for `snap` and emit it.
///
/// `record` is what a previous flow saw of this same hand, `start_kyoku`
/// first (see [`super::journal`]). When given, the history must begin with
/// exactly those events — they are emitted as recorded — and only what
/// happened after them is inferred. A record the snapshot cannot extend is
/// dropped and the whole hand inferred, as without one.
///
/// The `m<n>` lists are tried newest-first (how the server sends them),
/// then as listed; the riichi-wait rule is tried on, then off. The first
/// history found wins.
pub fn rebuild(snap: &Snapshot, record: Option<&[MjaiEvent]>) -> Result<Rebuilt> {
    let n = snap.num_players;
    ensure!(n == 3 || n == 4, "{n} players");
    ensure!(
        snap.seats.len() == n as usize,
        "{} seats for {n} players",
        snap.seats.len()
    );
    ensure!(
        snap.seat < n && snap.oya < n,
        "seat {} / dealer {}",
        snap.seat,
        snap.oya
    );
    ensure!(
        snap.scores.len() == n as usize,
        "{} scores",
        snap.scores.len()
    );
    ensure!(!snap.dora_markers.is_empty(), "no dora indicator");
    ensure!(!snap.hand.is_empty(), "no hand");

    let us = snap.seat as usize;
    let our_steps = steps_of(snap.seat, n, &snap.seats[us].melds, false)?;
    // A kita is followed by a replacement draw, so it leaves the hand's
    // size alone; every other call or kan sets three tiles aside.
    let sets = our_steps
        .iter()
        .filter(|s| !matches!(s, Step::Kita(_)))
        .count();
    let our_turn = (snap.hand.len() + 3 * sets) % 3 == 2;
    let riichi = snap.seats[us].riichi_at.is_some();
    let drawn = our_turn.then(|| drawn_tile(&snap.hand, riichi));
    let waits = if riichi {
        let closed: Vec<u32> = snap
            .hand
            .iter()
            .copied()
            .filter(|t| Some(*t) != drawn)
            .collect();
        waits_of(&closed).filter(|waits| !furiten(snap, waits))
    } else {
        None
    };

    if let Some(record) = record.filter(|r| matches!(r.first(), Some(MjaiEvent::StartKyoku { .. })))
    {
        let record = trim_record(record);
        let forced = forced_acts(record);
        if let Some((steps, trace, next)) = search(snap, our_turn, waits, &forced) {
            match synthesize_after(snap, &steps, &trace, record, forced.len(), next, drawn) {
                Ok(rebuilt) => return Ok(rebuilt),
                Err(e) => tracing::debug!(
                    target: "akagi::bridge::tenhou",
                    "the recorded hand fits the snapshot but cannot be carried on ({e:#}); inferring the whole hand"
                ),
            }
        } else {
            tracing::debug!(
                target: "akagi::bridge::tenhou",
                "the recorded hand ({} events) does not fit the snapshot; inferring the whole hand",
                record.len() - 1
            );
        }
    }

    // Rejoined before anyone has done anything: the dealer is yet to make
    // its first draw. (Our own deal ends on our turn and is searched.)
    let untouched = snap
        .seats
        .iter()
        .all(|s| s.river.is_empty() && s.melds.is_empty());
    if untouched && !our_turn {
        let no_steps = vec![Vec::new(); n as usize];
        return synthesize(snap, &no_steps, &[], Next::Draw(snap.oya), None);
    }

    match search(snap, our_turn, waits, &[]) {
        Some((steps, trace, next)) => synthesize(snap, &steps, &trace, next, drawn),
        None => bail!("no order of play fits the snapshot"),
    }
}

/// The search over every meld-list reading and both riichi-wait settings,
/// with the history pinned to begin with `forced`.
fn search(
    snap: &Snapshot,
    our_turn: bool,
    waits: Option<[bool; 34]>,
    forced: &[Forced],
) -> Option<(Vec<Vec<Step>>, Vec<Act>, Next)> {
    let n = snap.num_players;
    let reorderable = snap.seats.iter().any(|s| s.melds.len() > 1);
    let orders: &[bool] = if reorderable { &[true, false] } else { &[true] };
    let guards: &[bool] = if waits.is_some() {
        &[true, false]
    } else {
        &[false]
    };
    for &newest_first in orders {
        let mut steps = Vec::with_capacity(n as usize);
        for (seat, s) in snap.seats.iter().enumerate() {
            steps.push(steps_of(seat as u8, n, &s.melds, newest_first).ok()?);
        }
        for &guard in guards {
            let mut search = Search {
                n,
                us: snap.seat,
                rivers: snap.seats.iter().map(|s| s.river.as_slice()).collect(),
                riichi_at: snap.seats.iter().map(|s| s.riichi_at).collect(),
                steps: &steps,
                our_turn,
                waits: if guard { waits } else { None },
                forced,
                trace: Vec::new(),
                failed: HashSet::new(),
                visits: 0,
                next: None,
            };
            let start = Pos {
                cur: snap.oya,
                entry: Entry::Draw,
                river: [0; 4],
                steps: [0; 4],
                upgraded: [0; 4],
                riichi: 0,
            };
            if search.turn(start) {
                let next = search.next.expect("a found history sets where it ends");
                let trace = search.trace;
                return Some((steps, trace, next));
            }
        }
    }
    None
}

/// A record cut where it stops mid-declaration: a riichi whose tile was
/// never seen is not part of what was recorded.
fn trim_record(record: &[MjaiEvent]) -> &[MjaiEvent] {
    match record.last() {
        Some(MjaiEvent::Reach { .. }) => &record[..record.len() - 1],
        _ => record,
    }
}

/// One recorded act the history must reproduce, in the search's terms.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Forced {
    Draw(u8),
    Discard {
        seat: u8,
        pai: String,
        riichi: bool,
    },
    Call {
        seat: u8,
        kind: CallKind,
        pai: String,
    },
    Ankan(u8),
    Kakan(u8),
    Kita(u8),
}

/// The acts a record (`start_kyoku` first) pins the history to.
fn forced_acts(record: &[MjaiEvent]) -> Vec<Forced> {
    let mut out = Vec::new();
    let mut declaring = [false; 4];
    for event in &record[1..] {
        match event {
            MjaiEvent::Tsumo { actor, .. } => out.push(Forced::Draw(*actor)),
            MjaiEvent::Reach { actor, .. } => declaring[*actor as usize % 4] = true,
            MjaiEvent::Dahai { actor, pai, .. } => out.push(Forced::Discard {
                seat: *actor,
                pai: pai.clone(),
                riichi: std::mem::take(&mut declaring[*actor as usize % 4]),
            }),
            MjaiEvent::Chi { actor, pai, .. } => out.push(Forced::Call {
                seat: *actor,
                kind: CallKind::Chi,
                pai: pai.clone(),
            }),
            MjaiEvent::Pon { actor, pai, .. } => out.push(Forced::Call {
                seat: *actor,
                kind: CallKind::Pon,
                pai: pai.clone(),
            }),
            MjaiEvent::Daiminkan { actor, pai, .. } => out.push(Forced::Call {
                seat: *actor,
                kind: CallKind::Daiminkan,
                pai: pai.clone(),
            }),
            MjaiEvent::Ankan { actor, .. } => out.push(Forced::Ankan(*actor)),
            MjaiEvent::Kakan { actor, .. } => out.push(Forced::Kakan(*actor)),
            MjaiEvent::Kita { actor, .. } => out.push(Forced::Kita(*actor)),
            _ => {}
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallKind {
    Chi,
    Pon,
    Daiminkan,
}

/// One seat's call or kita, in the order it made them.
#[derive(Debug, Clone)]
enum Step {
    /// A call on `target`'s discard `tile`; `own` came from the caller's
    /// hand. `added` is set for a pon that later became an added kan.
    Call {
        kind: CallKind,
        target: u8,
        tile: u32,
        own: Vec<u32>,
        added: Option<u32>,
    },
    Ankan([u32; 4]),
    Kita(u32),
}

/// A seat's `m<n>` list as steps, oldest first.
fn steps_of(seat: u8, n: u8, melds: &[SnapMeld], newest_first: bool) -> Result<Vec<Step>> {
    let mut out = Vec::with_capacity(melds.len());
    for m in melds {
        let step = match m {
            SnapMeld::Kita(t) => {
                ensure!(n == 3, "kita in a four-player hand");
                Step::Kita(*t)
            }
            SnapMeld::Call(m) => {
                let target = (seat + m.target_rel) % 4;
                if m.kind != MeldKind::Ankan {
                    ensure!(
                        m.target_rel != 0 && target < n,
                        "seat {seat} calls from an empty seat"
                    );
                }
                match m.kind {
                    MeldKind::Chi | MeldKind::Pon => Step::Call {
                        kind: if m.kind == MeldKind::Chi {
                            CallKind::Chi
                        } else {
                            CallKind::Pon
                        },
                        target,
                        tile: m.tiles[0],
                        own: m.tiles[1..3].to_vec(),
                        added: None,
                    },
                    MeldKind::Daiminkan => Step::Call {
                        kind: CallKind::Daiminkan,
                        target,
                        tile: m.tiles[0],
                        own: m.tiles[1..4].to_vec(),
                        added: None,
                    },
                    // The pon underneath an added kan: `tiles` is the added
                    // tile, then the pon with its called tile first.
                    MeldKind::Kakan => Step::Call {
                        kind: CallKind::Pon,
                        target,
                        tile: m.tiles[1],
                        own: m.tiles[2..4].to_vec(),
                        added: Some(m.tiles[0]),
                    },
                    MeldKind::Ankan => {
                        Step::Ankan([m.tiles[0], m.tiles[1], m.tiles[2], m.tiles[3]])
                    }
                }
            }
        };
        out.push(step);
    }
    if newest_first {
        out.reverse();
    }
    Ok(out)
}

/// The tile we drew this turn, when the hand holds one. In riichi it is the
/// one whose removal leaves a ready hand (riichi locked the rest); otherwise
/// nothing depends on it.
fn drawn_tile(hand: &[u32], riichi: bool) -> u32 {
    let last = *hand.last().expect("a hand owing a discard is not empty");
    if !riichi {
        return last;
    }
    hand.iter()
        .rev()
        .copied()
        .find(|&t| {
            let rest: Vec<u32> = hand.iter().copied().filter(|&x| x != t).collect();
            waits_of(&rest).is_some()
        })
        .unwrap_or(last)
}

/// The tile kinds that complete `closed`, or `None` if it is not ready.
fn waits_of(closed: &[u32]) -> Option<[bool; 34]> {
    let mut counts = [0u8; 34];
    for &t in closed {
        counts[(t / 4) as usize] += 1;
    }
    let len_div3 = ((closed.len() + 1) / 3) as u8;
    let mut waits = [false; 34];
    for kind in 0..34 {
        if counts[kind] >= 4 {
            continue;
        }
        counts[kind] += 1;
        waits[kind] = riichienv_core::shanten::calc_shanten_from_counts(&counts, len_div3) == -1;
        counts[kind] -= 1;
    }
    waits.iter().any(|&w| w).then_some(waits)
}

/// Whether we are furiten on `waits`: one of our own discards, including
/// one another seat called, completes the hand. Then others' tiles we wait
/// on could pass after our riichi without a win.
fn furiten(snap: &Snapshot, waits: &[bool; 34]) -> bool {
    let us = snap.seat;
    let ours = snap.seats[us as usize].river.iter().copied();
    let called = snap.seats.iter().enumerate().flat_map(|(seat, s)| {
        s.melds.iter().filter_map(move |m| match m {
            SnapMeld::Call(m)
                if m.kind != MeldKind::Ankan && (seat as u8 + m.target_rel) % 4 == us =>
            {
                Some(if m.kind == MeldKind::Kakan {
                    m.tiles[1]
                } else {
                    m.tiles[0]
                })
            }
            _ => None,
        })
    });
    ours.chain(called).any(|t| waits[(t / 4) as usize])
}

/// How a seat came to hold the turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Entry {
    /// It draws from the wall.
    Draw,
    /// It called a chi or pon and discards without drawing.
    AfterCall,
    /// It called an open kan and draws a replacement.
    AfterKan,
}

/// A point between turns. Everything the search can still do depends on
/// nothing else, so a position that failed once fails again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Pos {
    cur: u8,
    entry: Entry,
    /// River tiles each seat has discarded so far.
    river: [u8; 4],
    /// Steps each seat has made so far.
    steps: [u8; 4],
    /// Per seat, bit `i`: step `i` (a pon) has become an added kan.
    upgraded: [u16; 4],
    /// Bit `s`: seat `s` has declared riichi.
    riichi: u8,
}

#[derive(Debug, Clone, Copy)]
enum Act {
    Draw {
        seat: u8,
    },
    Discard {
        seat: u8,
        tile: u32,
        riichi: bool,
        after_call: bool,
    },
    Call {
        seat: u8,
        step: usize,
    },
    Ankan {
        seat: u8,
        step: usize,
    },
    Kakan {
        seat: u8,
        step: usize,
    },
    Kita {
        seat: u8,
        step: usize,
    },
}

struct Search<'a> {
    n: u8,
    us: u8,
    rivers: Vec<&'a [u32]>,
    riichi_at: Vec<Option<usize>>,
    steps: &'a [Vec<Step>],
    /// Our hand holds a draw or a fresh call: the snapshot ends on our turn.
    our_turn: bool,
    /// While set and we are in riichi, others may not discard these kinds.
    waits: Option<[bool; 34]>,
    /// The history must begin with these acts (a recorded part of the hand).
    forced: &'a [Forced],
    trace: Vec<Act>,
    failed: HashSet<Pos>,
    visits: usize,
    next: Option<Next>,
}

impl Search<'_> {
    /// The next act is still one the record pins down.
    fn in_record(&self) -> bool {
        self.trace.len() < self.forced.len()
    }

    /// Append `act` to the history, unless the record says something else
    /// happened at this point.
    fn push(&mut self, act: Act) -> bool {
        if let Some(forced) = self.forced.get(self.trace.len()) {
            let fits = match (forced, act) {
                (Forced::Draw(s), Act::Draw { seat }) => *s == seat,
                (
                    Forced::Discard { seat, pai, riichi },
                    Act::Discard {
                        seat: s,
                        tile,
                        riichi: r,
                        ..
                    },
                ) => *seat == s && *riichi == r && *pai == tenhou_to_mjai_one(tile),
                (Forced::Call { seat, kind, pai }, Act::Call { seat: s, step }) => {
                    *seat == s
                        && matches!(&self.steps[s as usize][step],
                            Step::Call { kind: k, tile, .. }
                                if k == kind && tenhou_to_mjai_one(*tile) == *pai)
                }
                (Forced::Ankan(s), Act::Ankan { seat, .. })
                | (Forced::Kakan(s), Act::Kakan { seat, .. })
                | (Forced::Kita(s), Act::Kita { seat, .. }) => *s == seat,
                _ => false,
            };
            if !fits {
                return false;
            }
        }
        self.trace.push(act);
        true
    }

    /// Everything in the snapshot has happened.
    fn done(&self, pos: &Pos) -> bool {
        (0..self.n as usize).all(|s| {
            pos.river[s] as usize == self.rivers[s].len()
                && pos.steps[s] as usize == self.steps[s].len()
                && self.steps[s].iter().enumerate().all(|(i, st)| {
                    !matches!(st, Step::Call { added: Some(_), .. })
                        || pos.upgraded[s] & (1 << i) != 0
                })
                && (self.riichi_at[s].is_none() || pos.riichi & (1 << s) != 0)
        })
    }

    /// `pos.cur` takes its turn.
    fn turn(&mut self, pos: Pos) -> bool {
        if self.visits >= VISIT_LIMIT || self.failed.contains(&pos) {
            return false;
        }
        self.visits += 1;
        let mark = self.trace.len();
        let drew = pos.entry != Entry::AfterCall;
        if drew && !self.push(Act::Draw { seat: pos.cur }) {
            return false;
        }
        if self.act(pos, drew, pos.entry == Entry::AfterKan) {
            return true;
        }
        self.trace.truncate(mark);
        self.failed.insert(pos);
        false
    }

    /// `pos.cur` holds the turn: the snapshot ends here, it discards, or —
    /// with a drawn tile — it kans or sets aside kita first. `rinshan`: the
    /// tile it holds is a replacement draw.
    fn act(&mut self, pos: Pos, drew: bool, rinshan: bool) -> bool {
        let s = pos.cur;
        let si = s as usize;
        if self.done(&pos) && !self.in_record() {
            // On our turn the snapshot holds our draw or call. Another seat
            // can be caught holding a replacement draw: its kan or kita is
            // on the table, the draw that followed is not. (Its ordinary
            // draw leaves no trace; that snapshot ends before it.)
            let ends_here = if s == self.us {
                self.our_turn
            } else {
                !self.our_turn && rinshan
            };
            if ends_here {
                self.next = Some(Next::Discard(s));
                return true;
            }
        }
        if self.discard(pos) {
            return true;
        }
        if !drew {
            return false;
        }
        let mark = self.trace.len();
        let q = pos.steps[si] as usize;
        let own = match self.steps[si].get(q) {
            Some(Step::Ankan(_)) => Some(Act::Ankan { seat: s, step: q }),
            Some(Step::Kita(_)) => Some(Act::Kita { seat: s, step: q }),
            _ => None,
        };
        if let Some(own) = own {
            let mut next = pos;
            next.steps[si] += 1;
            if self.push(own) && self.push(Act::Draw { seat: s }) && self.act(next, true, true) {
                return true;
            }
            self.trace.truncate(mark);
        }
        if pos.riichi & (1 << s) == 0 {
            for i in 0..q {
                let pending = matches!(self.steps[si][i], Step::Call { added: Some(_), .. })
                    && pos.upgraded[si] & (1 << i) == 0;
                if !pending {
                    continue;
                }
                let mut next = pos;
                next.upgraded[si] |= 1 << i;
                if self.push(Act::Kakan { seat: s, step: i })
                    && self.push(Act::Draw { seat: s })
                    && self.act(next, true, true)
                {
                    return true;
                }
                self.trace.truncate(mark);
            }
        }
        false
    }

    /// `pos.cur` discards: its next river tile, or a tile a caller took.
    fn discard(&mut self, pos: Pos) -> bool {
        let s = pos.cur;
        let si = s as usize;
        let mark = self.trace.len();
        let declaring =
            pos.riichi & (1 << s) == 0 && self.riichi_at[si] == Some(pos.river[si] as usize);
        let after_call = pos.entry == Entry::AfterCall;
        let forbidden = if after_call {
            self.kuikae(si, pos.steps[si] as usize - 1)
        } else {
            [None; 2]
        };

        if let Some(&tile) = self.rivers[si].get(pos.river[si] as usize) {
            if self.may_discard(&pos, tile, &forbidden) {
                let mut next = pos;
                next.river[si] += 1;
                if declaring {
                    next.riichi |= 1 << s;
                }
                let pushed = self.push(Act::Discard {
                    seat: s,
                    tile,
                    riichi: declaring,
                    after_call,
                });
                if pushed && self.passed(next) {
                    return true;
                }
                self.trace.truncate(mark);
            }
        }

        for x in 0..self.n {
            let xi = x as usize;
            if x == s || pos.riichi & (1 << x) != 0 {
                continue;
            }
            let q = pos.steps[xi] as usize;
            let Some(&Step::Call {
                kind, target, tile, ..
            }) = self.steps[xi].get(q)
            else {
                continue;
            };
            if target != s || !self.may_discard(&pos, tile, &forbidden) {
                continue;
            }
            let mut next = pos;
            if declaring {
                next.riichi |= 1 << s;
            }
            next.steps[xi] += 1;
            next.cur = x;
            next.entry = if kind == CallKind::Daiminkan {
                Entry::AfterKan
            } else {
                Entry::AfterCall
            };
            let pushed = self.push(Act::Discard {
                seat: s,
                tile,
                riichi: declaring,
                after_call,
            }) && self.push(Act::Call { seat: x, step: q });
            if !pushed {
                self.trace.truncate(mark);
                continue;
            }
            // The snapshot may end with another seat thinking over the
            // discard it owes after its call.
            if x != self.us
                && !self.our_turn
                && kind != CallKind::Daiminkan
                && self.done(&next)
                && !self.in_record()
            {
                self.next = Some(Next::Discard(x));
                return true;
            }
            if self.turn(next) {
                return true;
            }
            self.trace.truncate(mark);
        }
        false
    }

    /// Nobody called the discard just made.
    fn passed(&mut self, mut pos: Pos) -> bool {
        let next_seat = (pos.cur + 1) % self.n;
        if !self.our_turn && self.done(&pos) && !self.in_record() {
            self.next = Some(Next::Draw(next_seat));
            return true;
        }
        pos.cur = next_seat;
        pos.entry = Entry::Draw;
        self.turn(pos)
    }

    fn may_discard(&self, pos: &Pos, tile: u32, forbidden: &[Option<u32>; 2]) -> bool {
        if self.in_record() {
            return true; // a recorded discard happened; no rule overrides it
        }
        let kind = tile / 4;
        if forbidden.contains(&Some(kind)) {
            return false;
        }
        match &self.waits {
            Some(waits) => {
                pos.cur == self.us || pos.riichi & (1 << self.us) == 0 || !waits[kind as usize]
            }
            None => true,
        }
    }

    /// Kinds the seat may not discard right after step `step` (Tenhou bars
    /// both the called kind and, for a chi from the end of a run, the kind
    /// on its other side).
    fn kuikae(&self, seat: usize, step: usize) -> [Option<u32>; 2] {
        let Step::Call {
            kind, tile, own, ..
        } = &self.steps[seat][step]
        else {
            return [None; 2];
        };
        let t = tile / 4;
        match kind {
            CallKind::Pon => [Some(t), None],
            CallKind::Chi => {
                let mut o = [own[0] / 4, own[1] / 4];
                o.sort_unstable();
                let other_side = if o == [t + 1, t + 2] && t % 9 <= 5 {
                    Some(t + 3)
                } else if t % 9 >= 3 && o == [t - 2, t - 1] {
                    Some(t - 3)
                } else {
                    None
                };
                [Some(t), other_side]
            }
            CallKind::Daiminkan => [None; 2],
        }
    }
}

/// Turn a history into mjai events, inventing our draws (see the module
/// docs) and checking that our hand comes out as the snapshot holds it.
fn synthesize(
    snap: &Snapshot,
    steps: &[Vec<Step>],
    trace: &[Act],
    next: Next,
    drawn: Option<u32>,
) -> Result<Rebuilt> {
    let us = snap.seat;
    let mj = |t: u32| tenhou_to_mjai_one(t);

    // Our draws: each takes the tile our next act uses.
    let mut our_draws: Vec<Option<u32>> = vec![None; trace.len()];
    for (i, act) in trace.iter().enumerate() {
        if !matches!(act, Act::Draw { seat } if *seat == us) {
            continue;
        }
        let used = match trace.get(i + 1) {
            Some(Act::Discard { tile, .. }) => Some(*tile),
            Some(Act::Ankan { step, .. }) => match &steps[us as usize][*step] {
                Step::Ankan(tiles) => Some(tiles[0]),
                _ => None,
            },
            Some(Act::Kakan { step, .. }) => match &steps[us as usize][*step] {
                Step::Call { added, .. } => *added,
                _ => None,
            },
            Some(Act::Kita { step, .. }) => match &steps[us as usize][*step] {
                Step::Kita(t) => Some(*t),
                _ => None,
            },
            None => drawn,
            Some(_) => None,
        };
        let Some(tile) = used else {
            bail!("our draw at step {i} is followed by nothing it could be");
        };
        our_draws[i] = Some(tile);
    }

    // The opening hand: everything we ever held, less what we drew.
    let mut held: Vec<u32> = snap.hand.clone();
    for step in &steps[us as usize] {
        match step {
            Step::Call { own, added, .. } => {
                held.extend(own);
                held.extend(added);
            }
            Step::Ankan(tiles) => held.extend(tiles),
            Step::Kita(t) => held.push(*t),
        }
    }
    for act in trace {
        if let Act::Discard { seat, tile, .. } = act {
            if *seat == us {
                held.push(*tile);
            }
        }
    }
    let mut seen = HashSet::new();
    if let Some(dup) = held.iter().find(|t| !seen.insert(**t)) {
        bail!("tile {dup} would be held twice");
    }
    for tile in our_draws.iter().flatten() {
        let Some(pos) = held.iter().position(|t| t == tile) else {
            bail!("we would draw tile {tile} twice");
        };
        held.swap_remove(pos);
    }
    ensure!(
        held.len() == 13,
        "the opening hand comes out at {} tiles",
        held.len()
    );
    held.sort_unstable();
    check_our_hand(snap, steps, trace, &our_draws, &held)?;

    // Start-of-hand scores and sticks: this hand's riichi deposits back.
    let n = snap.num_players as usize;
    let mut scores = snap.scores.clone();
    let mut kyotaku = snap.kyotaku;
    for (seat, s) in snap.seats.iter().enumerate() {
        if s.riichi_at.is_some() {
            scores[seat] += 1000;
            kyotaku = kyotaku.saturating_sub(1);
        }
    }
    let mut tehais = vec![vec!["?".to_string(); 13]; n];
    tehais[us as usize] = held.iter().map(|&t| mj(t)).collect();

    let events = vec![MjaiEvent::StartKyoku {
        bakaze: snap.bakaze.clone(),
        dora_marker: mj(snap.dora_markers[0]),
        kyoku: snap.kyoku,
        honba: snap.honba,
        kyotaku,
        oya: snap.oya,
        scores,
        tehais,
        num_players: snap.num_players,
        catch_up: 0,
    }];
    let mut emitter = Emitter::new(snap, steps, events, &snap.dora_markers[1..], [false; 4]);
    let our_draws: Vec<Option<String>> = our_draws.iter().map(|d| d.map(mj)).collect();
    emitter.emit(trace, &our_draws, None)?;
    let (events, last_discard) = emitter.finish();

    let ends_on_our_draw = matches!(trace.last(), Some(Act::Draw { seat }) if *seat == us);
    Ok(Rebuilt {
        events,
        next,
        approximate: steps.iter().any(|s| !s.is_empty()),
        recorded: 0,
        last_discard,
        drawn: if ends_on_our_draw { drawn } else { None },
    })
}

/// Carry a recorded hand on to the snapshot: the record as it was emitted,
/// then the acts after it with our draws worked out from what our hand
/// gained since (`forced` acts of `trace` are the record's).
fn synthesize_after(
    snap: &Snapshot,
    steps: &[Vec<Step>],
    trace: &[Act],
    record: &[MjaiEvent],
    forced: usize,
    next: Next,
    drawn: Option<u32>,
) -> Result<Rebuilt> {
    let us = snap.seat;
    let mj = |t: u32| tenhou_to_mjai_one(t);
    let gap = &trace[forced..];

    // Our hand where the record stops, and what the record already settled.
    let MjaiEvent::StartKyoku { tehais, .. } = &record[0] else {
        bail!("a record starts with start_kyoku");
    };
    let mut hand: Vec<String> = tehais
        .get(us as usize)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("the record has no hand for seat {us}"))?;
    let mut in_riichi = [false; 4];
    let mut markers_used = 1;
    let mut last_draw: Option<String> = None;
    let mut last_discard = None;
    for event in &record[1..] {
        let ours = |actor: &u8| *actor == us;
        match event {
            MjaiEvent::Tsumo { actor, pai } if ours(actor) => {
                hand.push(pai.clone());
                last_draw = Some(pai.clone());
                continue;
            }
            MjaiEvent::Dahai { actor, pai, .. } => {
                if ours(actor) {
                    take_named(&mut hand, pai)?;
                }
                last_discard = Some((*actor, pai.clone()));
            }
            MjaiEvent::Chi {
                actor, consumed, ..
            }
            | MjaiEvent::Pon {
                actor, consumed, ..
            } if ours(actor) => {
                for t in consumed {
                    take_named(&mut hand, t)?;
                }
            }
            MjaiEvent::Daiminkan {
                actor, consumed, ..
            } if ours(actor) => {
                for t in consumed {
                    take_named(&mut hand, t)?;
                }
            }
            MjaiEvent::Ankan { actor, consumed } if ours(actor) => {
                for t in consumed {
                    take_named(&mut hand, t)?;
                }
            }
            MjaiEvent::Kakan { actor, pai, .. } if ours(actor) => take_named(&mut hand, pai)?,
            MjaiEvent::Kita { actor, .. } if ours(actor) => take_named(&mut hand, "N")?,
            MjaiEvent::ReachAccepted { actor } => in_riichi[*actor as usize % 4] = true,
            MjaiEvent::Dora { .. } => markers_used += 1,
            _ => {}
        }
        last_draw = None;
    }

    // Our draws since: the tiles our hand now holds that it did not, plus
    // whatever it gave up meanwhile.
    let mut gained: Vec<String> = snap.hand.iter().map(|&t| mj(t)).collect();
    for act in gap {
        match *act {
            Act::Discard { seat, tile, .. } if seat == us => gained.push(mj(tile)),
            Act::Call { seat, step } if seat == us => {
                if let Step::Call { own, .. } = &steps[us as usize][step] {
                    gained.extend(own.iter().map(|&t| mj(t)));
                }
            }
            Act::Ankan { seat, step } if seat == us => {
                if let Step::Ankan(tiles) = &steps[us as usize][step] {
                    gained.extend(tiles.iter().map(|&t| mj(t)));
                }
            }
            Act::Kakan { seat, step } if seat == us => {
                if let Step::Call { added: Some(t), .. } = &steps[us as usize][step] {
                    gained.push(mj(*t));
                }
            }
            Act::Kita { seat, step } if seat == us => {
                if let Step::Kita(t) = &steps[us as usize][step] {
                    gained.push(mj(*t));
                }
            }
            _ => {}
        }
    }
    for t in &hand {
        take_named(&mut gained, t)?;
    }
    let draws = gap
        .iter()
        .filter(|a| matches!(a, Act::Draw { seat } if *seat == us))
        .count();
    ensure!(
        gained.len() == draws,
        "our hand gained {} tile(s) over {draws} draw(s)",
        gained.len()
    );

    // Each draw is the tile its turn then uses, when that is one we gained;
    // otherwise one we kept (the snapshot's own pick for a draw still held).
    let mut our_draws: Vec<Option<String>> = vec![None; gap.len()];
    for i in 0..gap.len() {
        if !matches!(gap[i], Act::Draw { seat } if seat == us) {
            continue;
        }
        let used: Vec<String> = match gap.get(i + 1) {
            Some(Act::Discard { tile, .. }) => vec![mj(*tile)],
            Some(Act::Ankan { step, .. }) => match &steps[us as usize][*step] {
                Step::Ankan(tiles) => tiles.iter().map(|&t| mj(t)).collect(),
                _ => Vec::new(),
            },
            Some(Act::Kakan { step, .. }) => match &steps[us as usize][*step] {
                Step::Call { added: Some(t), .. } => vec![mj(*t)],
                _ => Vec::new(),
            },
            Some(Act::Kita { step, .. }) => match &steps[us as usize][*step] {
                Step::Kita(t) => vec![mj(*t)],
                _ => Vec::new(),
            },
            None => drawn.map(mj).into_iter().collect(),
            Some(_) => Vec::new(),
        };
        let pick = used
            .iter()
            .find_map(|u| gained.iter().position(|g| g == u))
            .or(if gained.is_empty() { None } else { Some(0) })
            .ok_or_else(|| anyhow::anyhow!("no tile left for our draw"))?;
        our_draws[i] = Some(gained.remove(pick));
    }

    // Play our part forward: every tile in hand when used, and the hand as
    // the snapshot holds it at the end.
    let mut sim = hand.clone();
    for (i, act) in gap.iter().enumerate() {
        match *act {
            Act::Draw { seat } if seat == us => sim.push(our_draws[i].clone().unwrap_or_default()),
            Act::Discard { seat, tile, .. } if seat == us => take_named(&mut sim, &mj(tile))?,
            Act::Call { seat, step } if seat == us => {
                if let Step::Call { own, .. } = &steps[us as usize][step] {
                    for &t in own {
                        take_named(&mut sim, &mj(t))?;
                    }
                }
            }
            Act::Ankan { seat, step } if seat == us => {
                if let Step::Ankan(tiles) = &steps[us as usize][step] {
                    for &t in tiles {
                        take_named(&mut sim, &mj(t))?;
                    }
                }
            }
            Act::Kakan { seat, step } if seat == us => {
                if let Step::Call { added: Some(t), .. } = &steps[us as usize][step] {
                    take_named(&mut sim, &mj(*t))?;
                }
            }
            Act::Kita { seat, step } if seat == us => {
                if let Step::Kita(t) = &steps[us as usize][step] {
                    take_named(&mut sim, &mj(*t))?;
                }
            }
            _ => {}
        }
    }
    let mut want: Vec<String> = snap.hand.iter().map(|&t| mj(t)).collect();
    sim.sort();
    want.sort();
    ensure!(
        sim == want,
        "carried on, our hand ends as {sim:?}, not {want:?}"
    );

    let markers = snap.dora_markers.get(markers_used..).unwrap_or_default();
    let mut emitter = Emitter::new(snap, steps, record.to_vec(), markers, in_riichi);
    emitter.emit(gap, &our_draws, last_draw.clone())?;
    let (events, gap_discard) = emitter.finish();

    // The draw we hold, as one of the snapshot's tiles.
    let held_draw = match trace.last() {
        Some(Act::Draw { seat }) if *seat == us => {
            let name = if gap.is_empty() {
                last_draw
            } else {
                our_draws.last().cloned().flatten()
            };
            name.and_then(|name| {
                drawn
                    .filter(|&d| mj(d) == name)
                    .or_else(|| snap.hand.iter().rev().copied().find(|&t| mj(t) == name))
            })
        }
        _ => None,
    };
    Ok(Rebuilt {
        events,
        next,
        approximate: gap.iter().any(|a| {
            matches!(
                a,
                Act::Call { .. } | Act::Ankan { .. } | Act::Kakan { .. } | Act::Kita { .. }
            )
        }),
        recorded: record.len() - 1,
        last_discard: gap_discard.or(last_discard),
        drawn: held_draw,
    })
}

/// Remove one tile named `name`.
fn take_named(tiles: &mut Vec<String>, name: &str) -> Result<()> {
    let Some(pos) = tiles.iter().position(|t| t == name) else {
        bail!("{name} is used before it is held");
    };
    tiles.swap_remove(pos);
    Ok(())
}

/// Writes a history out as mjai events, after whatever `events` holds
/// already: kan indicators where they turn, riichi around its discard, the
/// tsumogiri flags that can be known.
struct Emitter<'a> {
    snap: &'a Snapshot,
    steps: &'a [Vec<Step>],
    events: Vec<MjaiEvent>,
    markers: std::slice::Iter<'a, u32>,
    /// Open and added kans whose indicator is still owed: where their kan
    /// event sits, so a snapshot that already shows the indicator can put
    /// it there.
    owed_dora: [Vec<usize>; 4],
    in_riichi: [bool; 4],
    last_discard: Option<(u8, String)>,
}

impl<'a> Emitter<'a> {
    fn new(
        snap: &'a Snapshot,
        steps: &'a [Vec<Step>],
        events: Vec<MjaiEvent>,
        markers: &'a [u32],
        in_riichi: [bool; 4],
    ) -> Self {
        Self {
            snap,
            steps,
            events,
            markers: markers.iter(),
            owed_dora: Default::default(),
            in_riichi,
            last_discard: None,
        }
    }

    /// `our_draws[i]` names our draw at `acts[i]`; `prev_draw` is our draw
    /// just before `acts`, if the history so far ends on one.
    fn emit(
        &mut self,
        acts: &[Act],
        our_draws: &[Option<String>],
        mut prev_draw: Option<String>,
    ) -> Result<()> {
        let us = self.snap.seat;
        let mj = |t: u32| tenhou_to_mjai_one(t);
        for (i, act) in acts.iter().enumerate() {
            match *act {
                Act::Draw { seat } => {
                    let pai = our_draws[i].clone().unwrap_or_else(|| "?".to_string());
                    prev_draw = (seat == us).then(|| pai.clone());
                    self.events.push(MjaiEvent::Tsumo { actor: seat, pai });
                    continue;
                }
                Act::Discard {
                    seat,
                    tile,
                    riichi,
                    after_call,
                } => {
                    let pai = mj(tile);
                    let tsumogiri = if seat == us {
                        prev_draw.as_deref() == Some(pai.as_str())
                    } else {
                        !after_call && self.in_riichi[seat as usize]
                    };
                    if riichi {
                        self.events.push(MjaiEvent::Reach {
                            actor: seat,
                            pai: None,
                        });
                    }
                    self.events.push(MjaiEvent::Dahai {
                        actor: seat,
                        pai: pai.clone(),
                        tsumogiri,
                    });
                    if riichi {
                        self.events.push(MjaiEvent::ReachAccepted { actor: seat });
                        self.in_riichi[seat as usize] = true;
                    }
                    // An open or added kan turns its indicator after the discard.
                    for _ in std::mem::take(&mut self.owed_dora[seat as usize]) {
                        if let Some(&m) = self.markers.next() {
                            self.events.push(MjaiEvent::Dora { dora_marker: mj(m) });
                        }
                    }
                    self.last_discard = Some((seat, pai));
                }
                Act::Call { seat, step } => {
                    let Step::Call {
                        kind,
                        target,
                        tile,
                        own,
                        ..
                    } = &self.steps[seat as usize][step]
                    else {
                        bail!("call step {step} of seat {seat} is not a call");
                    };
                    let (actor, target, pai) = (seat, *target, mj(*tile));
                    let event = match kind {
                        CallKind::Chi => MjaiEvent::Chi {
                            actor,
                            target,
                            pai,
                            consumed: [mj(own[0]), mj(own[1])],
                        },
                        CallKind::Pon => MjaiEvent::Pon {
                            actor,
                            target,
                            pai,
                            consumed: [mj(own[0]), mj(own[1])],
                        },
                        CallKind::Daiminkan => {
                            self.owed_dora[seat as usize].push(self.events.len());
                            MjaiEvent::Daiminkan {
                                actor,
                                target,
                                pai,
                                consumed: [mj(own[0]), mj(own[1]), mj(own[2])],
                            }
                        }
                    };
                    self.events.push(event);
                }
                Act::Ankan { seat, step } => {
                    let Step::Ankan(tiles) = &self.steps[seat as usize][step] else {
                        bail!("ankan step {step} of seat {seat} is not a closed kan");
                    };
                    self.events.push(MjaiEvent::Ankan {
                        actor: seat,
                        consumed: tiles.map(mj),
                    });
                    // A closed kan turns its indicator at once.
                    if let Some(&m) = self.markers.next() {
                        self.events.push(MjaiEvent::Dora { dora_marker: mj(m) });
                    }
                }
                Act::Kakan { seat, step } => {
                    let Step::Call {
                        tile,
                        own,
                        added: Some(added),
                        ..
                    } = &self.steps[seat as usize][step]
                    else {
                        bail!("kakan step {step} of seat {seat} is not an added kan");
                    };
                    self.owed_dora[seat as usize].push(self.events.len());
                    self.events.push(MjaiEvent::Kakan {
                        actor: seat,
                        pai: mj(*added),
                        consumed: [mj(*tile), mj(own[0]), mj(own[1])],
                    });
                }
                Act::Kita { seat, .. } => self.events.push(MjaiEvent::Kita {
                    actor: seat,
                    pai: Some("N".to_string()),
                }),
            }
            prev_draw = None;
        }
        Ok(())
    }

    /// The events, with any indicator the snapshot already shows for a kan
    /// whose discard is still to come placed right after that kan (so the
    /// history still ends on the turn's last draw or call), and the last
    /// discard written.
    fn finish(mut self) -> (Vec<MjaiEvent>, Option<(u8, String)>) {
        let mut late: Vec<usize> = self.owed_dora.into_iter().flatten().collect();
        late.sort_unstable();
        let late: Vec<(usize, u32)> = late.into_iter().zip(self.markers.copied()).collect();
        // From the back, so the earlier positions hold.
        for &(at, m) in late.iter().rev() {
            self.events.insert(
                at + 1,
                MjaiEvent::Dora {
                    dora_marker: tenhou_to_mjai_one(m),
                },
            );
        }
        (self.events, self.last_discard)
    }
}

/// Play our part of the history forward from the opening hand and make
/// sure every tile is in hand when used and the hand ends as the snapshot
/// has it.
fn check_our_hand(
    snap: &Snapshot,
    steps: &[Vec<Step>],
    trace: &[Act],
    our_draws: &[Option<u32>],
    opening: &[u32],
) -> Result<()> {
    fn take(hand: &mut Vec<u32>, tile: u32) -> Result<()> {
        let Some(pos) = hand.iter().position(|&t| t == tile) else {
            bail!("tile {tile} is used before we hold it");
        };
        hand.swap_remove(pos);
        Ok(())
    }
    let us = snap.seat;
    let mut hand: Vec<u32> = opening.to_vec();
    for (i, act) in trace.iter().enumerate() {
        match *act {
            Act::Draw { seat } if seat == us => {
                hand.push(our_draws[i].expect("our draws are assigned"));
            }
            Act::Discard { seat, tile, .. } if seat == us => take(&mut hand, tile)?,
            Act::Call { seat, step } if seat == us => {
                if let Step::Call { own, .. } = &steps[us as usize][step] {
                    for &t in own {
                        take(&mut hand, t)?;
                    }
                }
            }
            Act::Ankan { seat, step } if seat == us => {
                if let Step::Ankan(tiles) = &steps[us as usize][step] {
                    for &t in tiles {
                        take(&mut hand, t)?;
                    }
                }
            }
            Act::Kakan { seat, step } if seat == us => {
                if let Step::Call { added: Some(t), .. } = &steps[us as usize][step] {
                    take(&mut hand, *t)?;
                }
            }
            Act::Kita { seat, step } if seat == us => {
                if let Step::Kita(t) = &steps[us as usize][step] {
                    take(&mut hand, *t)?;
                }
            }
            _ => {}
        }
    }
    let mut want = snap.hand.clone();
    hand.sort_unstable();
    want.sort_unstable();
    ensure!(
        hand == want,
        "the rebuilt hand ends as {hand:?}, not {want:?}"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KINDS: [&str; 34] = [
        "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p", "5p", "6p",
        "7p", "8p", "9p", "1s", "2s", "3s", "4s", "5s", "6s", "7s", "8s", "9s", "E", "S", "W", "N",
        "P", "F", "C",
    ];

    /// Tenhou index of copy `copy` (0..=3) of `kind`. Copy 0 of a five is
    /// the red one.
    fn tile(kind: &str, copy: u32) -> u32 {
        let k = KINDS.iter().position(|k| *k == kind).expect("tile kind") as u32;
        k * 4 + copy
    }

    fn kind(name: &str) -> u32 {
        tile(name, 0) / 4
    }

    // `m` bitfields, the inverse of `meld::Meld::parse`.

    /// Pon of `name`, leaving copy `unused` out; the called tile is the
    /// `r`-th of the other three.
    fn pon(name: &str, unused: u32, r: u32, target_rel: u32) -> SnapMeld {
        let m = ((kind(name) * 3 + r) << 9) | (unused << 5) | 0x8 | target_rel;
        SnapMeld::Call(Meld::parse(m))
    }

    /// Added kan on a pon of `name`: copy `added` was added, the called
    /// tile is the `r`-th of the other three.
    fn kakan(name: &str, added: u32, r: u32, target_rel: u32) -> SnapMeld {
        let m = ((kind(name) * 3 + r) << 9) | (added << 5) | 0x10 | target_rel;
        SnapMeld::Call(Meld::parse(m))
    }

    /// Chi from the left of the run starting at `start` (a suited tile 1..7),
    /// copies `v`, the `r`-th tile called.
    fn chi(start: &str, v: [u32; 3], r: u32) -> SnapMeld {
        let k = kind(start);
        let t7 = (k / 9) * 7 + k % 9;
        let m = ((t7 * 3 + r) << 10) | (v[2] << 7) | (v[1] << 5) | (v[0] << 3) | 0x4 | 3;
        SnapMeld::Call(Meld::parse(m))
    }

    fn ankan(name: &str) -> SnapMeld {
        SnapMeld::Call(Meld::parse(tile(name, 1) << 8))
    }

    fn seat(river: &[u32], riichi_at: Option<usize>, melds: Vec<SnapMeld>) -> SeatSnapshot {
        SeatSnapshot {
            river: river.to_vec(),
            riichi_at,
            melds,
        }
    }

    fn snapshot(seat: u8, oya: u8, hand: Vec<u32>, seats: Vec<SeatSnapshot>) -> Snapshot {
        let n = seats.len() as u8;
        let mut scores = vec![if n == 3 { 35_000 } else { 25_000 }; n as usize];
        let mut kyotaku = 0;
        for (i, s) in seats.iter().enumerate() {
            if s.riichi_at.is_some() {
                scores[i] -= 1000;
                kyotaku += 1;
            }
        }
        Snapshot {
            num_players: n,
            seat,
            oya,
            bakaze: "E".into(),
            kyoku: oya + 1,
            honba: 0,
            kyotaku,
            scores,
            dora_markers: vec![tile("C", 3)],
            hand,
            seats,
        }
    }

    /// The history as short tokens: `T0` draw, `D0:5m` discard (`*` =
    /// tsumogiri), `R0`/`RA0` riichi, `P1<0:5m` pon (`C` chi, `K` open
    /// kan), `AK3`/`KK2` closed/added kan, `N1` kita, `dora:C`.
    fn tokens(r: &Rebuilt) -> Vec<String> {
        r.events[1..]
            .iter()
            .map(|e| match e {
                MjaiEvent::Tsumo { actor, .. } => format!("T{actor}"),
                MjaiEvent::Dahai {
                    actor,
                    pai,
                    tsumogiri,
                } => format!("D{actor}:{pai}{}", if *tsumogiri { "*" } else { "" }),
                MjaiEvent::Reach { actor, .. } => format!("R{actor}"),
                MjaiEvent::ReachAccepted { actor } => format!("RA{actor}"),
                MjaiEvent::Pon {
                    actor, target, pai, ..
                } => format!("P{actor}<{target}:{pai}"),
                MjaiEvent::Chi {
                    actor, target, pai, ..
                } => format!("C{actor}<{target}:{pai}"),
                MjaiEvent::Daiminkan {
                    actor, target, pai, ..
                } => format!("K{actor}<{target}:{pai}"),
                MjaiEvent::Ankan { actor, .. } => format!("AK{actor}"),
                MjaiEvent::Kakan { actor, .. } => format!("KK{actor}"),
                MjaiEvent::Kita { actor, .. } => format!("N{actor}"),
                MjaiEvent::Dora { dora_marker } => format!("dora:{dora_marker}"),
                other => panic!("unexpected {other:?}"),
            })
            .collect()
    }

    fn start(r: &Rebuilt) -> (Vec<Vec<String>>, Vec<i32>, u8) {
        match &r.events[0] {
            MjaiEvent::StartKyoku {
                tehais,
                scores,
                kyotaku,
                ..
            } => (tehais.clone(), scores.clone(), *kyotaku),
            other => panic!("expected start_kyoku, got {other:?}"),
        }
    }

    fn sorted_mjai(tiles: &[u32]) -> Vec<String> {
        let mut t = tiles.to_vec();
        t.sort_unstable();
        t.iter().map(|&t| tenhou_to_mjai_one(t)).collect()
    }

    /// 13 tiles none of the scenarios below discard or call.
    fn plain_hand(copy: u32) -> Vec<u32> {
        let mut h: Vec<u32> = [
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p",
        ]
        .iter()
        .map(|k| tile(k, copy))
        .collect();
        h.push(tile("5p", copy));
        h
    }

    #[test]
    fn without_calls_the_order_is_the_rotation_from_the_dealer() {
        let hand = plain_hand(1);
        let snap = snapshot(
            0,
            2,
            hand.clone(),
            vec![
                seat(&[tile("E", 0), tile("S", 0)], None, vec![]),
                seat(&[tile("6s", 1)], None, vec![]),
                seat(&[tile("1s", 1), tile("2s", 1)], None, vec![]),
                seat(&[tile("3s", 1), tile("4s", 1)], None, vec![]),
            ],
        );
        let r = rebuild(&snap, None).unwrap();
        assert_eq!(
            tokens(&r),
            [
                "T2", "D2:1s", "T3", "D3:3s", "T0", "D0:E*", "T1", "D1:6s", "T2", "D2:2s", "T3",
                "D3:4s", "T0", "D0:S*"
            ]
        );
        assert_eq!(r.next, Next::Draw(1));
        assert!(!r.approximate);
        assert_eq!(r.last_discard, Some((0, "S".to_string())));
        let (tehais, scores, kyotaku) = start(&r);
        assert_eq!(tehais[0], sorted_mjai(&hand), "every draw was discarded");
        assert_eq!(tehais[1], vec!["?"; 13]);
        assert_eq!(scores, vec![25_000; 4]);
        assert_eq!(kyotaku, 0);
        // Our draws are the tiles we discarded.
        assert!(matches!(&r.events[5], MjaiEvent::Tsumo { actor: 0, pai } if pai == "E"));
    }

    #[test]
    fn a_hand_holding_a_draw_ends_on_our_own_turn() {
        let mut hand = plain_hand(1);
        hand.push(tile("9p", 1));
        let snap = snapshot(
            0,
            0,
            hand.clone(),
            vec![
                seat(&[tile("E", 0)], None, vec![]),
                seat(&[tile("S", 0)], None, vec![]),
                seat(&[tile("W", 0)], None, vec![]),
                seat(&[tile("N", 0)], None, vec![]),
            ],
        );
        let r = rebuild(&snap, None).unwrap();
        assert_eq!(
            tokens(&r),
            ["T0", "D0:E*", "T1", "D1:S", "T2", "D2:W", "T3", "D3:N", "T0"]
        );
        assert_eq!(r.next, Next::Discard(0));
        assert!(
            matches!(r.events.last(), Some(MjaiEvent::Tsumo { actor: 0, pai }) if pai == "9p"),
            "the draw in hand is the last tile: {:?}",
            r.events.last()
        );
        let (tehais, ..) = start(&r);
        assert_eq!(tehais[0], sorted_mjai(&hand[..13]));
    }

    /// A pon on the left neighbour's discard skips nobody; with nothing else
    /// to go on it lands as late as it can.
    #[test]
    fn a_call_from_the_left_lands_as_late_as_it_can() {
        let snap = snapshot(
            2,
            0,
            plain_hand(2),
            vec![
                seat(&[tile("1s", 1), tile("2s", 1)], None, vec![]),
                // Pon of P from rel 3 (the left), copy 3 unused, copy 0 called.
                seat(
                    &[tile("7s", 1), tile("8s", 1), tile("9s", 1)],
                    None,
                    vec![pon("P", 3, 0, 3)],
                ),
                seat(&[tile("E", 1), tile("S", 1)], None, vec![]),
                seat(&[tile("W", 1), tile("N", 1)], None, vec![]),
            ],
        );
        let r = rebuild(&snap, None).unwrap();
        assert_eq!(
            tokens(&r),
            [
                "T0", "D0:1s", "T1", "D1:7s", "T2", "D2:E*", "T3", "D3:W", "T0", "D0:2s", "T1",
                "D1:8s", "T2", "D2:S*", "T3", "D3:N", "T0", "D0:P", "P1<0:P", "D1:9s"
            ]
        );
        assert_eq!(r.next, Next::Draw(2));
        assert!(r.approximate);
    }

    /// Chi of 3m with 4m5m bars 3m and 6m as the next discard. The latest
    /// placement would make the 6m that discard, so the chi moves earlier.
    #[test]
    fn the_discard_after_a_chi_is_never_a_swap_call_tile() {
        let snap = snapshot(
            2,
            0,
            plain_hand(2),
            vec![
                seat(&[tile("1s", 1), tile("2s", 1)], None, vec![]),
                seat(
                    &[tile("7s", 1), tile("8s", 1), tile("6m", 3)],
                    None,
                    vec![chi("3m", [3, 3, 3], 0)],
                ),
                seat(&[tile("E", 1), tile("S", 1)], None, vec![]),
                seat(&[tile("W", 1), tile("N", 1)], None, vec![]),
            ],
        );
        let r = rebuild(&snap, None).unwrap();
        assert_eq!(
            tokens(&r),
            [
                "T0", "D0:1s", "T1", "D1:7s", "T2", "D2:E*", "T3", "D3:W", "T0", "D0:3m",
                "C1<0:3m", "D1:8s", "T2", "D2:S*", "T3", "D3:N", "T0", "D0:2s", "T1", "D1:6m"
            ]
        );
        assert_eq!(r.next, Next::Draw(2));
    }

    /// We are in riichi waiting on 2p/5p, and a pon from across the table
    /// took a 2p. Had that 2p been thrown after our riichi we would have won
    /// on it, so the pon must come before the riichi — the only history that
    /// puts it there, though the latest placement would not.
    #[test]
    fn nothing_we_wait_on_passes_after_our_riichi() {
        // 123456789m 11p 34p: waiting on 2p / 5p.
        let hand: Vec<u32> = vec![
            tile("1m", 0),
            tile("2m", 0),
            tile("3m", 0),
            tile("4m", 0),
            tile("5m", 1),
            tile("6m", 0),
            tile("7m", 0),
            tile("8m", 0),
            tile("9m", 0),
            tile("1p", 0),
            tile("1p", 1),
            tile("3p", 0),
            tile("4p", 0),
        ];
        let snap = snapshot(
            0,
            0,
            hand,
            vec![
                seat(&[tile("E", 0), tile("S", 0), tile("W", 0)], Some(1), vec![]),
                seat(&[tile("N", 0), tile("P", 0)], None, vec![]),
                seat(&[tile("F", 0), tile("C", 0)], None, vec![]),
                // Pon of 2p from rel 2 (across): copy 0 called.
                seat(
                    &[tile("E", 1), tile("S", 1), tile("W", 1)],
                    None,
                    vec![pon("2p", 3, 0, 2)],
                ),
            ],
        );
        let r = rebuild(&snap, None).unwrap();
        assert_eq!(
            tokens(&r),
            [
                "T0", "D0:E*", "T1", "D1:2p", "P3<1:2p", "D3:E", "T0", "R0", "D0:S*", "RA0", "T1",
                "D1:N", "T2", "D2:F", "T3", "D3:S", "T0", "D0:W*", "T1", "D1:P", "T2", "D2:C",
                "T3", "D3:W"
            ]
        );
        assert_eq!(r.next, Next::Draw(0));
        let (_, scores, kyotaku) = start(&r);
        assert_eq!(scores, vec![25_000; 4], "the riichi deposit is given back");
        assert_eq!(kyotaku, 0);

        // Without our riichi nothing holds the pon back from its latest
        // place, after seat 1's last river tile.
        let mut open = snap.clone();
        open.seats[0].riichi_at = None;
        open.scores[0] += 1000;
        open.kyotaku = 0;
        let t = tokens(&rebuild(&open, None).unwrap());
        assert_eq!(&t[t.len() - 3..], ["D1:2p", "P3<1:2p", "D3:W"]);

        // Nor when we are furiten: with a 5p of our own in the river, a 2p
        // could pass after our riichi without ending the hand.
        let mut furiten = snap.clone();
        furiten.seats[0].river[0] = tile("5p", 1);
        let t = tokens(&rebuild(&furiten, None).unwrap());
        assert_eq!(&t[t.len() - 3..], ["D1:2p", "P3<1:2p", "D3:W"]);
    }

    #[test]
    fn our_own_pon_takes_its_tiles_from_the_opening_hand() {
        // Ten concealed tiles after the pon.
        let hand: Vec<u32> = ["1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p"]
            .iter()
            .map(|k| tile(k, 2))
            .collect();
        let snap = snapshot(
            1,
            0,
            hand.clone(),
            vec![
                seat(&[tile("1s", 1), tile("2s", 1)], None, vec![]),
                seat(
                    &[tile("7s", 1), tile("8s", 1), tile("9s", 1)],
                    None,
                    vec![pon("P", 3, 0, 3)],
                ),
                seat(&[tile("E", 1), tile("S", 1)], None, vec![]),
                seat(&[tile("W", 1), tile("N", 1)], None, vec![]),
            ],
        );
        let r = rebuild(&snap, None).unwrap();
        let t = tokens(&r);
        assert_eq!(
            &t[t.len() - 2..],
            ["P1<0:P", "D1:9s"],
            "the post-call discard is a tedashi"
        );
        assert!(t.contains(&"D1:7s*".to_string()));
        let (tehais, ..) = start(&r);
        let mut opening = hand;
        opening.extend([tile("P", 1), tile("P", 2), tile("9s", 1)]);
        assert_eq!(tehais[1], sorted_mjai(&opening));
    }

    #[test]
    fn a_closed_kan_turns_its_indicator_before_the_replacement_draw() {
        let mut snap = snapshot(
            0,
            0,
            plain_hand(2),
            vec![
                seat(&[tile("E", 0)], None, vec![]),
                seat(&[tile("S", 0)], None, vec![]),
                seat(&[tile("W", 0)], None, vec![]),
                seat(&[tile("N", 0)], None, vec![ankan("9p")]),
            ],
        );
        snap.dora_markers.push(tile("C", 2));
        let r = rebuild(&snap, None).unwrap();
        assert_eq!(
            tokens(&r),
            ["T0", "D0:E*", "T1", "D1:S", "T2", "D2:W", "T3", "AK3", "dora:C", "T3", "D3:N"]
        );
        assert_eq!(r.next, Next::Draw(0));
    }

    /// An added kan needs a draw after its pon, which decides where the pon
    /// goes; its indicator turns after the discard that follows.
    #[test]
    fn an_added_kan_needs_a_later_turn_of_its_own() {
        let mut snap = snapshot(
            0,
            0,
            plain_hand(2),
            vec![
                seat(&[tile("E", 0), tile("S", 0)], None, vec![]),
                seat(&[tile("W", 0)], None, vec![]),
                // Pon of P from rel 3 (the left), later added to with copy 3.
                seat(
                    &[tile("N", 0), tile("F", 0)],
                    None,
                    vec![kakan("P", 3, 0, 3)],
                ),
                seat(&[tile("E", 1), tile("S", 1)], None, vec![]),
            ],
        );
        snap.dora_markers.push(tile("C", 2));
        let r = rebuild(&snap, None).unwrap();
        assert_eq!(
            tokens(&r),
            [
                "T0", "D0:E*", "T1", "D1:P", "P2<1:P", "D2:N", "T3", "D3:E", "T0", "D0:S*", "T1",
                "D1:W", "T2", "KK2", "T2", "D2:F", "dora:C", "T3", "D3:S"
            ]
        );
    }

    #[test]
    fn sanma_rotates_over_three_seats_and_sets_kita_aside() {
        let snap = snapshot(
            0,
            0,
            plain_hand(2),
            vec![
                seat(&[tile("E", 0)], None, vec![]),
                seat(&[tile("S", 0)], None, vec![SnapMeld::Kita(tile("N", 0))]),
                seat(&[tile("W", 0)], None, vec![]),
            ],
        );
        let r = rebuild(&snap, None).unwrap();
        assert_eq!(
            tokens(&r),
            ["T0", "D0:E*", "T1", "N1", "T1", "D1:S", "T2", "D2:W"]
        );
        assert_eq!(r.next, Next::Draw(0));
        let (tehais, scores, _) = start(&r);
        assert_eq!(tehais.len(), 3);
        assert_eq!(scores, vec![35_000; 3]);
    }

    /// The pon must come first (the 6m after it cannot follow the chi of
    /// 3m), whichever way round the frame lists the two calls.
    #[test]
    fn a_seat_with_two_calls_is_read_in_either_listed_order() {
        let seats = |melds: Vec<SnapMeld>| {
            vec![
                seat(&[], None, vec![]),
                seat(&[tile("6m", 3), tile("7s", 1)], None, melds),
                seat(&[tile("E", 1), tile("S", 1)], None, vec![]),
                seat(&[tile("W", 1), tile("N", 1)], None, vec![]),
            ]
        };
        let expected = [
            "T0", "D0:F", "P1<0:F", "D1:6m", "T2", "D2:E*", "T3", "D3:W", "T0", "D0:3m", "C1<0:3m",
            "D1:7s", "T2", "D2:S*", "T3", "D3:N",
        ];
        for melds in [
            vec![chi("3m", [3, 3, 3], 0), pon("F", 3, 0, 3)],
            vec![pon("F", 3, 0, 3), chi("3m", [3, 3, 3], 0)],
        ] {
            let r = rebuild(&snapshot(2, 0, plain_hand(2), seats(melds)), None).unwrap();
            assert_eq!(tokens(&r), expected);
        }
    }

    #[test]
    fn in_riichi_the_draw_in_hand_is_the_tile_outside_the_ready_hand() {
        // 123456789m 11p 34p (waiting on 2p / 5p) plus a drawn 2m.
        let mut hand: Vec<u32> = vec![
            tile("1m", 0),
            tile("2m", 0),
            tile("2m", 1),
            tile("3m", 0),
            tile("4m", 0),
            tile("5m", 1),
            tile("6m", 0),
            tile("7m", 0),
            tile("8m", 0),
            tile("9m", 0),
            tile("1p", 0),
            tile("1p", 1),
            tile("3p", 0),
            tile("4p", 0),
        ];
        hand.sort_unstable();
        let snap = snapshot(
            0,
            0,
            hand,
            vec![
                seat(&[tile("E", 0)], Some(0), vec![]),
                seat(&[tile("S", 0)], None, vec![]),
                seat(&[tile("W", 0)], None, vec![]),
                seat(&[tile("N", 0)], None, vec![]),
            ],
        );
        let r = rebuild(&snap, None).unwrap();
        assert_eq!(r.next, Next::Discard(0));
        assert!(
            matches!(r.events.last(), Some(MjaiEvent::Tsumo { actor: 0, pai }) if pai == "2m"),
            "{:?}",
            r.events.last()
        );
        assert_eq!(&tokens(&r)[..4], ["T0", "R0", "D0:E*", "RA0"]);
    }

    /// A kita is followed by a replacement draw: it leaves the hand's size
    /// alone, so 13 tiles after one kita is not our turn and 14 is.
    #[test]
    fn our_kita_leaves_the_hand_size_alone() {
        let seats = || {
            vec![
                seat(&[tile("E", 0)], None, vec![SnapMeld::Kita(tile("N", 0))]),
                seat(&[tile("S", 0)], None, vec![]),
                seat(&[tile("W", 0)], None, vec![]),
            ]
        };
        let r = rebuild(&snapshot(0, 0, plain_hand(2), seats()), None).unwrap();
        assert_eq!(
            tokens(&r),
            ["T0", "N0", "T0", "D0:E*", "T1", "D1:S", "T2", "D2:W"]
        );
        assert_eq!(r.next, Next::Draw(0));
        assert_eq!(r.drawn, None);
        let (tehais, ..) = start(&r);
        assert_eq!(tehais[0], sorted_mjai(&plain_hand(2)));

        let mut hand = plain_hand(2);
        hand.push(tile("9p", 2));
        let r = rebuild(&snapshot(0, 0, hand, seats()), None).unwrap();
        assert_eq!(r.next, Next::Discard(0));
        assert_eq!(r.drawn, Some(tile("9p", 2)));
    }

    /// Another seat can be caught holding its replacement draw: its kan is
    /// on the table, the discard after it is not.
    #[test]
    fn another_seat_may_hold_its_replacement_draw() {
        let mut snap = snapshot(
            0,
            0,
            plain_hand(2),
            vec![
                seat(&[tile("E", 0)], None, vec![]),
                seat(&[tile("S", 0)], None, vec![]),
                seat(&[tile("W", 0)], None, vec![]),
                seat(&[], None, vec![ankan("9p")]),
            ],
        );
        snap.dora_markers.push(tile("C", 2));
        let r = rebuild(&snap, None).unwrap();
        assert_eq!(
            tokens(&r),
            ["T0", "D0:E*", "T1", "D1:S", "T2", "D2:W", "T3", "AK3", "dora:C", "T3"]
        );
        assert_eq!(r.next, Next::Discard(3));
    }

    #[test]
    fn a_rejoin_before_the_first_draw_waits_for_the_dealer() {
        let empty = || seat(&[], None, vec![]);
        let r = rebuild(
            &snapshot(
                0,
                2,
                plain_hand(2),
                vec![empty(), empty(), empty(), empty()],
            ),
            None,
        )
        .unwrap();
        assert_eq!(r.events.len(), 1, "start_kyoku alone");
        assert_eq!(r.next, Next::Draw(2));
    }

    /// We added to our pon and hold the replacement draw, and the frame
    /// already shows the new indicator. It goes right after the kan, so the
    /// history still ends on our draw.
    #[test]
    fn an_indicator_already_shown_goes_after_its_kan() {
        let hand: Vec<u32> = plain_hand(2)[..11].to_vec();
        let mut snap = snapshot(
            2,
            0,
            hand.clone(),
            vec![
                seat(&[tile("E", 0), tile("S", 0)], None, vec![]),
                seat(&[tile("W", 0)], None, vec![]),
                seat(&[tile("N", 0)], None, vec![kakan("P", 3, 0, 3)]),
                seat(&[tile("F", 0)], None, vec![]),
            ],
        );
        snap.dora_markers.push(tile("C", 2));
        let r = rebuild(&snap, None).unwrap();
        assert_eq!(
            tokens(&r),
            [
                "T0", "D0:E", "T1", "D1:P", "P2<1:P", "D2:N", "T3", "D3:F", "T0", "D0:S", "T1",
                "D1:W", "T2", "KK2", "dora:C", "T2"
            ]
        );
        assert_eq!(r.next, Next::Discard(2));
        assert_eq!(r.drawn, Some(hand[10]));
    }

    /// A history that ends on our pon holds no draw.
    #[test]
    fn a_history_ending_on_our_call_holds_no_draw() {
        let hand: Vec<u32> = plain_hand(2)[..11].to_vec();
        let snap = snapshot(
            1,
            0,
            hand,
            vec![
                seat(&[tile("1s", 1), tile("2s", 1)], None, vec![]),
                seat(
                    &[tile("7s", 1), tile("8s", 1)],
                    None,
                    vec![pon("P", 3, 0, 3)],
                ),
                seat(&[tile("E", 1), tile("S", 1)], None, vec![]),
                seat(&[tile("W", 1), tile("N", 1)], None, vec![]),
            ],
        );
        let r = rebuild(&snap, None).unwrap();
        assert_eq!(tokens(&r).last().map(String::as_str), Some("P1<0:P"));
        assert_eq!(r.next, Next::Discard(1));
        assert_eq!(r.drawn, None);
    }

    #[test]
    fn an_empty_hand_is_refused_not_a_panic() {
        let empty = || seat(&[], None, vec![]);
        let snap = snapshot(0, 0, Vec::new(), vec![empty(), empty(), empty()]);
        assert!(rebuild(&snap, None).is_err());
    }

    fn tsumo(actor: u8, pai: &str) -> MjaiEvent {
        MjaiEvent::Tsumo {
            actor,
            pai: pai.into(),
        }
    }

    fn dahai(actor: u8, pai: &str, tsumogiri: bool) -> MjaiEvent {
        MjaiEvent::Dahai {
            actor,
            pai: pai.into(),
            tsumogiri,
        }
    }

    /// The snapshot of `a_call_from_the_left_lands_as_late_as_it_can`.
    fn left_pon_snapshot() -> Snapshot {
        snapshot(
            2,
            0,
            plain_hand(2),
            vec![
                seat(&[tile("1s", 1), tile("2s", 1)], None, vec![]),
                seat(
                    &[tile("7s", 1), tile("8s", 1), tile("9s", 1)],
                    None,
                    vec![pon("P", 3, 0, 3)],
                ),
                seat(&[tile("E", 1), tile("S", 1)], None, vec![]),
                seat(&[tile("W", 1), tile("N", 1)], None, vec![]),
            ],
        )
    }

    /// A record of the hand pins what the snapshot leaves open: here the
    /// pon came on the first discard, not the last. The record goes out as
    /// recorded (its tsumogiri flags included) and only the rest is
    /// inferred.
    #[test]
    fn a_record_of_the_hand_pins_when_its_calls_happened() {
        let snap = left_pon_snapshot();
        let start = rebuild(&snap, None).unwrap().events[0].clone();
        let record = vec![
            start,
            tsumo(0, "?"),
            dahai(0, "P", false),
            MjaiEvent::Pon {
                actor: 1,
                target: 0,
                pai: "P".into(),
                consumed: ["P".into(), "P".into()],
            },
            dahai(1, "7s", false),
            tsumo(2, "E"),
            dahai(2, "E", true),
            tsumo(3, "?"),
            dahai(3, "W", true),
        ];
        let r = rebuild(&snap, Some(&record)).unwrap();
        assert_eq!(
            tokens(&r),
            [
                "T0", "D0:P", "P1<0:P", "D1:7s", "T2", "D2:E*", "T3", "D3:W*", "T0", "D0:1s", "T1",
                "D1:8s", "T2", "D2:S*", "T3", "D3:N", "T0", "D0:2s", "T1", "D1:9s"
            ]
        );
        assert_eq!(
            r.events[..record.len()],
            record[..],
            "the record as recorded"
        );
        assert_eq!(r.recorded, record.len() - 1);
        assert!(!r.approximate, "no call after the record");
        assert_eq!(r.next, Next::Draw(2));
        assert_eq!(r.last_discard, Some((1, "9s".to_string())));
    }

    /// A record the snapshot cannot extend (here: a discard no river holds)
    /// is dropped, and the hand inferred as without one.
    #[test]
    fn a_record_that_does_not_fit_is_dropped() {
        let snap = left_pon_snapshot();
        let without = rebuild(&snap, None).unwrap();
        let record = vec![
            without.events[0].clone(),
            tsumo(0, "?"),
            dahai(0, "9m", false),
        ];
        let r = rebuild(&snap, Some(&record)).unwrap();
        assert_eq!(r.recorded, 0);
        assert_eq!(tokens(&r), tokens(&without));
    }

    /// A record ending on a riichi declaration whose tile was never seen
    /// stops before the declaration; the inferred rest makes it again.
    #[test]
    fn a_record_cut_mid_declaration_is_trimmed() {
        let snap = left_pon_snapshot();
        let start = rebuild(&snap, None).unwrap().events[0].clone();
        let record = vec![
            start,
            MjaiEvent::Reach {
                actor: 0,
                pai: None,
            },
        ];
        assert_eq!(trim_record(&record).len(), 1);
    }

    #[test]
    fn a_snapshot_no_order_of_play_fits_is_refused() {
        // Our two discards against nobody else's: the turn never comes back.
        let snap = snapshot(
            0,
            0,
            plain_hand(2),
            vec![
                seat(&[tile("E", 0), tile("S", 0)], None, vec![]),
                seat(&[], None, vec![]),
                seat(&[], None, vec![]),
                seat(&[], None, vec![]),
            ],
        );
        assert!(rebuild(&snap, None).is_err());
    }

    #[test]
    fn a_tile_held_twice_is_refused() {
        // The pon's own tiles are also in our concealed hand.
        let mut hand: Vec<u32> = plain_hand(2)[..9].to_vec();
        hand.push(tile("P", 1));
        let snap = snapshot(
            1,
            0,
            hand,
            vec![
                seat(&[tile("1s", 1), tile("2s", 1)], None, vec![]),
                seat(
                    &[tile("7s", 1), tile("8s", 1), tile("9s", 1)],
                    None,
                    vec![pon("P", 3, 0, 3)],
                ),
                seat(&[tile("E", 1), tile("S", 1)], None, vec![]),
                seat(&[tile("W", 1), tile("N", 1)], None, vec![]),
            ],
        );
        assert!(rebuild(&snap, None).is_err());
    }
}
