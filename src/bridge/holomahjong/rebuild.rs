//! Rebuilding the hand in progress from a reconnect snapshot
//! (`OnChongxinLianjieChenggong`).
//!
//! A client that reconnects mid-hand gets the hand as a snapshot instead of
//! its history: the field, and per seat the score, the hand (ours in full,
//! the others' as a tile count), the calls, the kita tiles and the river. An
//! mjai consumer needs a history, so this module derives one that ends in
//! exactly that state.
//!
//! The river does most of the work. Each entry is the discard as it was
//! sent — `_` for a tsumogiri, `*`/`@` on a riichi declaration — and a
//! discard that was called keeps its place with the caller's direction mark
//! appended (`"p1-"`: the caller saw it come from its previous seat). So the
//! turn order is not a guess: from the dealer, a seat discards its next river
//! tile; an unmarked one passes the turn to the next seat, a marked one to its
//! caller, who called the matching meld and discards next.
//!
//! What is made up:
//! - Our draws. Every turn draws the tile it then discards (or kans, or sets
//!   aside as kita), and the opening hand is whatever makes that end at the
//!   hand we hold now. Every tile in it is one we really held.
//! - When a closed kan, an added kan or a kita happened: at that seat's first
//!   draw where it could have (an added kan after its pon). The dora
//!   indicators they revealed follow them in the order the field lists them.

use super::tile::{self, Meld};
use crate::schema::{mjai::Tile, MjaiEvent};
use serde_json::Value as JsonValue;

/// The hand in progress as a history.
#[derive(Debug, Clone, PartialEq)]
pub struct Rebuilt {
    /// mjai actor of each wire seat.
    pub actor_of: [Option<u8>; 4],
    pub my_actor: u8,
    pub hand_no: u8,
    /// `start_kyoku` followed by the history; `catch_up` already set.
    pub events: Vec<MjaiEvent>,
    /// Seat of the last discard, if the history ends right after one.
    pub last_discard_seat: Option<u8>,
}

struct RiverTile {
    pai: Tile,
    tsumogiri: bool,
    riichi: bool,
    /// Actor that called it.
    called_by: Option<u8>,
}

#[derive(Default)]
struct SeatSnap {
    seat: u8,
    score: i32,
    river: Vec<RiverTile>,
    /// Open calls in the order listed, with whether history has used them.
    calls: Vec<(Meld, bool)>,
    /// Closed and added kans, to place in the seat's own turns.
    own_turn: Vec<Meld>,
    kita: Vec<Tile>,
    /// Ours: the concealed tiles, a held draw last.
    hand: Option<Vec<Tile>>,
    concealed: usize,
}

/// `body` is `[_, our_wind, server_time, field, players, zimo, deadline]`.
pub fn rebuild(body: &JsonValue, n: u8) -> Result<Rebuilt, String> {
    let err = |what: &str| format!("snapshot: {what}");
    let my_wind = u8::try_from(
        body.get(1)
            .and_then(JsonValue::as_u64)
            .ok_or_else(|| err("our wind"))?,
    )
    .map_err(|_| err("our wind"))?;
    let field = body.get(3).ok_or_else(|| err("field"))?;
    let num =
        |v: Option<&JsonValue>, what: &str| v.and_then(JsonValue::as_u64).ok_or_else(|| err(what));
    let round_wind = num(field.get(0), "round wind")? as usize;
    let hand_no = u8::try_from(num(field.get(1), "hand number")?)
        .ok()
        .filter(|&h| h < n)
        .ok_or_else(|| err("hand number"))?;
    let honba = u8::try_from(num(field.get(2), "honba")?).map_err(|_| err("honba"))?;
    let sticks_now =
        super::kyotaku(field.get(3).ok_or_else(|| err("sticks"))?).ok_or_else(|| err("sticks"))?;
    let doras: Vec<Tile> = field
        .get(4)
        .and_then(JsonValue::as_array)
        .ok_or_else(|| err("dora"))?
        .iter()
        .map(|d| d.as_str().and_then(tile::tile))
        .collect::<Option<_>>()
        .filter(|d: &Vec<Tile>| !d.is_empty())
        .ok_or_else(|| err("dora"))?;

    let players = body
        .get(4)
        .and_then(JsonValue::as_array)
        .ok_or_else(|| err("players"))?;
    if players.len() != n as usize {
        return Err(err("player count"));
    }
    let mut actor_of = [None; 4];
    for p in players {
        let seat =
            super::seat_of(p.get(1).ok_or_else(|| err("seat"))?).ok_or_else(|| err("seat"))?;
        let wind = u8::try_from(num(p.get(2), "wind")?)
            .ok()
            .filter(|&w| w < n)
            .ok_or_else(|| err("wind"))?;
        if actor_of[seat as usize]
            .replace((wind + hand_no) % n)
            .is_some()
        {
            return Err(err("duplicate seat"));
        }
    }
    let mut seen = [false; 4];
    for a in actor_of.iter().flatten() {
        if std::mem::replace(&mut seen[*a as usize], true) {
            return Err(err("duplicate wind"));
        }
    }
    let actor = |seat: u8| actor_of.get(seat as usize).copied().flatten();
    let my_actor = (my_wind + hand_no) % n;

    let mut seats: Vec<SeatSnap> = (0..n).map(|_| SeatSnap::default()).collect();
    for p in players {
        let seat = super::seat_of(&p[1]).ok_or_else(|| err("seat"))?;
        let a = actor(seat).ok_or_else(|| err("seat"))?;
        let s = &mut seats[a as usize];
        s.seat = seat;
        s.score = i32::try_from(
            p.get(3)
                .and_then(JsonValue::as_i64)
                .ok_or_else(|| err("score"))?,
        )
        .map_err(|_| err("score"))?;
        let haisi = p
            .get(5)
            .and_then(JsonValue::as_str)
            .ok_or_else(|| err("hand"))?;
        let mut parts = haisi.split(',');
        let concealed = parts.next().unwrap_or("");
        if a == my_actor {
            let hand = tile::hand(concealed).ok_or_else(|| err("our hand"))?;
            s.concealed = hand.len();
            s.hand = Some(hand);
        } else {
            s.concealed = concealed.matches("x1").count();
        }
        for part in parts.filter(|p| !p.is_empty()) {
            if part.len() == 2 {
                s.kita.push(tile::tile(part).ok_or_else(|| err("kita"))?);
                continue;
            }
            match tile::meld(part).ok_or_else(|| err("meld"))? {
                m @ (Meld::Ankan { .. } | Meld::Kakan { .. }) => s.own_turn.push(m),
                m => s.calls.push((m, false)),
            }
        }
        for entry in p
            .get(6)
            .and_then(JsonValue::as_array)
            .ok_or_else(|| err("river"))?
        {
            s.river.push(
                river_tile(entry.as_str().ok_or_else(|| err("river"))?, seat, &actor)
                    .ok_or_else(|| err("river tile"))?,
            );
        }
    }

    // Scores and sticks at the deal: this hand's riichi deposits undone.
    let mut scores: Vec<i32> = seats.iter().map(|s| s.score).collect();
    let mut riichis = 0u8;
    for (a, s) in seats.iter().enumerate() {
        if s.river.iter().any(|r| r.riichi) {
            scores[a] += 1000;
            riichis += 1;
        }
    }
    let kyotaku = sticks_now
        .checked_sub(riichis)
        .ok_or_else(|| err("riichi sticks"))?;

    let mut b = Builder {
        n,
        seats,
        doras: doras[1..].to_vec(),
        events: Vec::new(),
    };
    let (ours, last_discard_seat) = b.walk(hand_no % n, my_actor)?;
    let history = b.events;
    let tehais = b.seats.iter().enumerate().map(|(a, s)| {
        if a as u8 == my_actor {
            opening_hand(s.hand.clone().unwrap_or_default(), a as u8, &history)
        } else {
            check_count(s.concealed, a as u8, &history).map(|()| vec!["?".to_string(); 13])
        }
    });
    let tehais = tehais.collect::<Result<Vec<_>, _>>()?;
    let count = history.len();
    let mut events = vec![MjaiEvent::StartKyoku {
        bakaze: super::BAKAZE
            .get(round_wind)
            .ok_or_else(|| err("round wind"))?
            .to_string(),
        dora_marker: doras[0].clone(),
        kyoku: hand_no + 1,
        honba,
        kyotaku,
        oya: hand_no % n,
        scores,
        tehais,
        num_players: n,
        // Our draw or call at the very end is the decision waiting now.
        catch_up: if ours { count.saturating_sub(1) } else { count },
    }];
    events.extend(history);
    Ok(Rebuilt {
        actor_of,
        my_actor,
        hand_no,
        events,
        last_discard_seat,
    })
}

/// `tile[_][*|@][+|=|-]`, the caller resolved to an actor.
fn river_tile(s: &str, discarder: u8, actor: &impl Fn(u8) -> Option<u8>) -> Option<RiverTile> {
    let (body, called_by) = match s.char_indices().last()? {
        (i, c @ ('+' | '=' | '-')) => {
            let from = match c {
                '+' => tile::Source::Next,
                '=' => tile::Source::Opposite,
                _ => tile::Source::Previous,
            };
            // The mark is the caller's view of this seat, on 4-seat geometry.
            let caller = (discarder + 4 - from.offset()) % 4;
            (&s[..i], Some(actor(caller)?))
        }
        _ => (s, None),
    };
    let d = tile::discard(body)?;
    Some(RiverTile {
        pai: d.pai,
        tsumogiri: d.tsumogiri,
        riichi: d.riichi,
        called_by,
    })
}

struct Builder {
    n: u8,
    seats: Vec<SeatSnap>,
    /// Indicators still to reveal, in order.
    doras: Vec<Tile>,
    events: Vec<MjaiEvent>,
}

impl Builder {
    /// Walk the turns from the dealer until the snapshot is reached. Returns
    /// whether the history ends on our pending decision, and the seat of a
    /// discard it ends on.
    fn walk(&mut self, oya: u8, me: u8) -> Result<(bool, Option<u8>), String> {
        let mut cursor = vec![0usize; self.n as usize];
        let mut cur = oya;
        // The seat to move took a chi or pon: it discards without drawing.
        let mut after_call = false;
        // The seat to move made an open kan: its next draw is the
        // replacement, and the kan's dora shows before its discard.
        let mut after_kan = false;
        let mut last_discard = None;
        loop {
            let a = cur as usize;
            let left = self.seats[a].river.len() - cursor[a];
            if !after_call {
                // A seat holds a draw when its concealed count is 3k+2.
                let holds = self.seats[a].concealed % 3 == 2;
                if left == 0 {
                    if holds {
                        self.draw_turn(cur, None);
                        if after_kan {
                            self.reveal_dora();
                        }
                        self.finish(&cursor)?;
                        return Ok((cur == me, None));
                    }
                    self.finish(&cursor)?;
                    return Ok((false, last_discard));
                }
                let next = self.seats[a].river[cursor[a]].pai.clone();
                self.draw_turn(cur, Some(next));
                if after_kan {
                    self.reveal_dora();
                }
            } else if left == 0 {
                self.finish(&cursor)?;
                return Ok((cur == me, None));
            }
            let r = &self.seats[a].river[cursor[a]];
            let (pai, tsumogiri, riichi, called_by) =
                (r.pai.clone(), r.tsumogiri, r.riichi, r.called_by);
            cursor[a] += 1;
            if riichi {
                self.events.push(MjaiEvent::Reach {
                    actor: cur,
                    pai: None,
                });
            }
            self.events.push(MjaiEvent::Dahai {
                actor: cur,
                pai: pai.clone(),
                tsumogiri,
            });
            if riichi {
                self.events.push(MjaiEvent::ReachAccepted { actor: cur });
            }
            last_discard = Some(self.seats[a].seat);
            match called_by {
                Some(caller) => {
                    let kan = self.call(caller, cur, &pai)?;
                    cur = caller;
                    after_call = !kan;
                    after_kan = kan;
                    last_discard = None;
                }
                None => {
                    cur = (cur + 1) % self.n;
                    after_call = false;
                    after_kan = false;
                }
            }
        }
    }

    /// A seat's draw, then any closed kan, added kan or kita it can make
    /// now (each followed by its replacement draw). `then` is the tile the
    /// turn discards next; `None` ends the history holding the last draw.
    fn draw_turn(&mut self, a: u8, then: Option<Tile>) {
        let ai = a as usize;
        // Own-turn moves possible now: kakan only once its pon is in.
        let mut moves: Vec<(Option<Meld>, Option<Tile>)> = Vec::new();
        let own = std::mem::take(&mut self.seats[ai].own_turn);
        let mut keep = Vec::new();
        for m in own {
            let ready = match &m {
                Meld::Kakan { consumed, .. } => self.events.iter().any(|e| {
                    matches!(e, MjaiEvent::Pon { actor, pai, consumed: c, .. }
                        if *actor == a && same_tiles(&[&[pai.clone()][..], &c[..]].concat(), consumed))
                }),
                _ => true,
            };
            if ready {
                moves.push((Some(m), None));
            } else {
                keep.push(m);
            }
        }
        self.seats[ai].own_turn = keep;
        for k in std::mem::take(&mut self.seats[ai].kita) {
            moves.push((None, Some(k)));
        }
        let mine = self.seats[ai].hand.is_some();
        let tile_for = |m: &(Option<Meld>, Option<Tile>)| match m {
            (Some(Meld::Ankan { consumed }), _) => consumed[0].clone(),
            (Some(Meld::Kakan { pai, .. }), _) => pai.clone(),
            (_, Some(k)) => k.clone(),
            _ => "?".to_string(),
        };
        // Each draw brings the tile the next step uses.
        let mut draws: Vec<Tile> = moves.iter().map(tile_for).collect();
        draws.push(match then {
            Some(t) => t,
            None => self.seats[ai]
                .hand
                .as_ref()
                .and_then(|h| h.last().cloned())
                .unwrap_or_else(|| "?".to_string()),
        });
        let mut draws = draws.into_iter();
        let draw = |t: Tile| MjaiEvent::Tsumo {
            actor: a,
            pai: if mine { t } else { "?".to_string() },
        };
        self.events.push(draw(draws.next().unwrap_or_default()));
        for m in moves {
            match m {
                (Some(Meld::Ankan { consumed }), _) => {
                    self.events.push(MjaiEvent::Ankan { actor: a, consumed });
                    self.reveal_dora();
                    self.events.push(draw(draws.next().unwrap_or_default()));
                }
                (Some(Meld::Kakan { pai, consumed }), _) => {
                    self.events.push(MjaiEvent::Kakan {
                        actor: a,
                        pai,
                        consumed,
                    });
                    self.events.push(draw(draws.next().unwrap_or_default()));
                    self.reveal_dora();
                }
                (_, Some(k)) => {
                    self.events.push(MjaiEvent::Kita {
                        actor: a,
                        pai: Some(k),
                    });
                    self.events.push(draw(draws.next().unwrap_or_default()));
                }
                _ => {}
            }
        }
    }

    /// `caller` calls `pai` discarded by `target`: its first unused open
    /// call on that tile. Returns whether it was an open kan.
    fn call(&mut self, caller: u8, target: u8, pai: &Tile) -> Result<bool, String> {
        let (meld, used) = self.seats[caller as usize]
            .calls
            .iter_mut()
            .find(|(m, used)| {
                !*used
                    && matches!(m, Meld::Chi { pai: p, .. } | Meld::Pon { pai: p, .. } | Meld::Daiminkan { pai: p, .. } if p == pai)
            })
            .ok_or_else(|| format!("snapshot: no call by actor {caller} on {pai}"))?;
        *used = true;
        let event = match meld.clone() {
            Meld::Chi { pai, consumed, .. } => MjaiEvent::Chi {
                actor: caller,
                target,
                pai,
                consumed,
            },
            Meld::Pon { pai, consumed, .. } => MjaiEvent::Pon {
                actor: caller,
                target,
                pai,
                consumed,
            },
            Meld::Daiminkan { pai, consumed, .. } => MjaiEvent::Daiminkan {
                actor: caller,
                target,
                pai,
                consumed,
            },
            _ => unreachable!("only open calls are matched"),
        };
        let kan = matches!(event, MjaiEvent::Daiminkan { .. });
        self.events.push(event);
        Ok(kan)
    }

    fn reveal_dora(&mut self) {
        if !self.doras.is_empty() {
            let dora_marker = self.doras.remove(0);
            self.events.push(MjaiEvent::Dora { dora_marker });
        }
    }

    /// Every river tile and every call used: the history reaches the
    /// snapshot exactly.
    fn finish(&self, cursor: &[usize]) -> Result<(), String> {
        for (a, s) in self.seats.iter().enumerate() {
            if cursor[a] != s.river.len() {
                return Err(format!("snapshot: seat {a} river not reached"));
            }
            if s.calls.iter().any(|(_, used)| !used) || !s.own_turn.is_empty() || !s.kita.is_empty()
            {
                return Err(format!(
                    "snapshot: seat {a} has calls the history cannot place"
                ));
            }
        }
        Ok(())
    }
}

fn same_tiles(a: &[Tile], b: &[Tile]) -> bool {
    let norm = |v: &[Tile]| {
        let mut v: Vec<String> = v
            .iter()
            .map(|t| t.trim_end_matches('r').to_string())
            .collect();
        v.sort();
        v
    };
    norm(a) == norm(b)
}

/// Our opening hand: the hand now, with our history undone.
fn opening_hand(mut hand: Vec<Tile>, me: u8, history: &[MjaiEvent]) -> Result<Vec<Tile>, String> {
    let take = |hand: &mut Vec<Tile>, t: &Tile| -> Result<(), String> {
        let i = hand
            .iter()
            .position(|h| h == t)
            .ok_or_else(|| format!("snapshot: our draw {t} not in hand"))?;
        hand.remove(i);
        Ok(())
    };
    for e in history.iter().rev() {
        match e {
            MjaiEvent::Tsumo { actor, pai } if *actor == me => take(&mut hand, pai)?,
            MjaiEvent::Dahai { actor, pai, .. } | MjaiEvent::Kakan { actor, pai, .. }
                if *actor == me =>
            {
                hand.push(pai.clone())
            }
            MjaiEvent::Kita {
                actor,
                pai: Some(pai),
            } if *actor == me => hand.push(pai.clone()),
            MjaiEvent::Chi {
                actor, consumed, ..
            }
            | MjaiEvent::Pon {
                actor, consumed, ..
            } if *actor == me => hand.extend(consumed.iter().cloned()),
            MjaiEvent::Daiminkan {
                actor, consumed, ..
            } if *actor == me => hand.extend(consumed.iter().cloned()),
            MjaiEvent::Ankan { actor, consumed } if *actor == me => {
                hand.extend(consumed.iter().cloned())
            }
            _ => {}
        }
    }
    if hand.len() != 13 {
        return Err(format!(
            "snapshot: our opening hand has {} tiles",
            hand.len()
        ));
    }
    Ok(hand)
}

/// Another seat's concealed count now must follow from 13 and its history.
fn check_count(now: usize, a: u8, history: &[MjaiEvent]) -> Result<(), String> {
    let mut count: i32 = 13;
    for e in history {
        count += match e {
            MjaiEvent::Tsumo { actor, .. } if *actor == a => 1,
            MjaiEvent::Dahai { actor, .. }
            | MjaiEvent::Kakan { actor, .. }
            | MjaiEvent::Kita { actor, .. }
                if *actor == a =>
            {
                -1
            }
            MjaiEvent::Chi { actor, .. } | MjaiEvent::Pon { actor, .. } if *actor == a => -2,
            MjaiEvent::Daiminkan { actor, .. } if *actor == a => -3,
            MjaiEvent::Ankan { actor, .. } if *actor == a => -4,
            _ => 0,
        };
    }
    if count != now as i32 {
        return Err(format!(
            "snapshot: seat {a} holds {now} tiles, history gives {count}"
        ));
    }
    Ok(())
}
