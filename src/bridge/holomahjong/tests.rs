//! Fixtures mirror the wire shapes (positional MessagePack arrays) with
//! made-up names and ids.

use super::*;
use serde_json::json;

fn to_mp(v: &JsonValue) -> rmpv::Value {
    match v {
        JsonValue::Null => rmpv::Value::Nil,
        JsonValue::Bool(b) => rmpv::Value::from(*b),
        JsonValue::Number(n) => n
            .as_i64()
            .map(rmpv::Value::from)
            .unwrap_or_else(|| rmpv::Value::from(n.as_f64().unwrap())),
        JsonValue::String(s) => rmpv::Value::from(s.as_str()),
        JsonValue::Array(a) => rmpv::Value::Array(a.iter().map(to_mp).collect()),
        JsonValue::Object(o) => rmpv::Value::Map(
            o.iter()
                .map(|(k, v)| (rmpv::Value::from(k.as_str()), to_mp(v)))
                .collect(),
        ),
    }
}

fn encode(v: rmpv::Value) -> Vec<u8> {
    let mut out = Vec::new();
    rmpv::encode::write_value(&mut out, &v).unwrap();
    out
}

fn push(method: i64, body: JsonValue) -> Vec<u8> {
    encode(rmpv::Value::Array(vec![
        rmpv::Value::from(method),
        to_mp(&body),
    ]))
}

const TIMER: [i64; 4] = [1_700_000_000_000, 1_700_000_020_000, 20_000, 30_000];

fn kaiju(my_seat: u8) -> Vec<u8> {
    let players: Vec<JsonValue> = (0..4u8)
        .map(|seat| {
            let me = seat == my_seat;
            let account = if me {
                json!("acct-fake-0001")
            } else {
                json!(null)
            };
            json!([
                0,
                [
                    seat,
                    account,
                    [
                        if me { "acct-fake-0001" } else { "" },
                        format!("Player{seat}"),
                        1
                    ],
                    !me,
                    !me
                ]
            ])
        })
        .collect();
    push(
        hub::ON_KAIJU,
        json!([
            null,
            [10000, null],
            my_seat,
            [null, 4, 25000, 25000],
            TIMER,
            players,
            {},
            [0, 0, 0, [0, 0, 0, 0, 0], null, 136]
        ]),
    )
}

/// `winds[seat]` is that seat's wind; `hands[seat]` its deal (`None` hidden).
fn qipai(me: u8, field: JsonValue, winds: [u8; 4], scores: [i32; 4], my_hand: &str) -> Vec<u8> {
    let players: Vec<JsonValue> = (0..4u8)
        .map(|seat| {
            let hand = if seat == me {
                my_hand.to_string()
            } else {
                "x1".repeat(13)
            };
            json!([
                null,
                seat,
                winds[seat as usize],
                scores[seat as usize],
                null,
                hand,
                [],
                0,
                0,
                []
            ])
        })
        .collect();
    push(
        hub::ON_QIPAI,
        json!([null, me, 1_700_000_000_000i64, null, field, players, TIMER]),
    )
}

fn field(round_wind: u8, hand_no: u8, honba: u8, sticks: u8, dora: &str) -> JsonValue {
    json!([
        round_wind,
        hand_no,
        honba,
        [0, 0, sticks, 0, 0],
        [dora],
        70,
        null,
        "fakehash",
        "fakesalt",
        false
    ])
}

const MY_DEAL: &str = "s2z5m4p7m8m1p3p8m5p7s6z2s9";

fn zimo(seat: u8, t: &str) -> Vec<u8> {
    push(
        hub::ON_ZIMO,
        json!([null, seat, t, null, null, null, [0], TIMER]),
    )
}

fn dapai(seat: u8, d: &str) -> Vec<u8> {
    push(
        hub::ON_DAPAI,
        json!([
            null,
            seat,
            d,
            [],
            [],
            [],
            [0],
            "",
            d.contains('*') as u8,
            "",
            TIMER
        ]),
    )
}

fn fulou(seat: u8, kind: u8, m: &str) -> Vec<u8> {
    push(hub::ON_FULOU, json!([null, seat, kind, m, TIMER]))
}

fn feed(b: &mut HoloMahjongBridge, frames: &[Vec<u8>]) -> Vec<JsonValue> {
    frames
        .iter()
        .flat_map(|f| b.parse(Direction::Down, f).events)
        .map(|e| serde_json::to_value(e).unwrap())
        .collect()
}

fn bridge() -> HoloMahjongBridge {
    HoloMahjongBridge::new(None, None)
}

#[test]
fn a_hand_with_a_chi_a_riichi_and_a_ron() {
    let mut b = bridge();
    let ev = feed(
        &mut b,
        &[
            kaiju(2),
            qipai(
                2,
                field(0, 0, 0, 0, "p5"),
                [0, 1, 2, 3],
                [25000; 4],
                MY_DEAL,
            ),
            zimo(0, ""),
            dapai(0, "z3_"),
            zimo(1, ""),
            dapai(1, "p1"),
            fulou(2, 1, "p1-23"),
            dapai(2, "z6"),
            zimo(3, ""),
            dapai(3, "m0"),
            zimo(0, ""),
            dapai(0, "m3"),
            zimo(1, ""),
            dapai(1, "p2*"),
            push(
                hub::ON_LIZHI,
                json!([null, 1, 1, [0, 0, 1, 0, 0], [25000, 24000, 25000, 25000]]),
            ),
            zimo(2, "s5"),
            dapai(2, "s5_"),
            push(
                hub::ON_HULE,
                json!([
                    null,
                    [[
                        1,
                        2,
                        "m2345677p678s123*",
                        "s5=",
                        30,
                        1,
                        "4",
                        [["立直", 2, "1"]],
                        8000,
                        null,
                        ["p5"],
                        ["p8"]
                    ]],
                    [
                        [0, 0, false, null],
                        [1, 9000, false, null],
                        [2, -8000, false, null],
                        [3, 0, false, null]
                    ],
                    TIMER
                ]),
            ),
        ],
    );
    let expected = vec![
        json!({"type": "start_game", "names": ["Player0", "Player1", "Player2", "Player3"], "id": 2, "num_players": 4}),
        json!({"type": "start_kyoku", "bakaze": "E", "dora_marker": "5p", "kyoku": 1, "honba": 0, "kyotaku": 0, "oya": 0,
               "scores": [25000, 25000, 25000, 25000], "num_players": 4,
               "tehais": [vec!["?"; 13], vec!["?"; 13],
                          vec!["2s", "P", "4m", "7p", "8m", "1m", "3p", "8p", "5m", "7p", "6s", "S", "9s"], vec!["?"; 13]]}),
        json!({"type": "tsumo", "actor": 0, "pai": "?"}),
        json!({"type": "dahai", "actor": 0, "pai": "W", "tsumogiri": true}),
        json!({"type": "tsumo", "actor": 1, "pai": "?"}),
        json!({"type": "dahai", "actor": 1, "pai": "1p", "tsumogiri": false}),
        json!({"type": "chi", "actor": 2, "target": 1, "pai": "1p", "consumed": ["2p", "3p"]}),
        json!({"type": "dahai", "actor": 2, "pai": "F", "tsumogiri": false}),
        json!({"type": "tsumo", "actor": 3, "pai": "?"}),
        json!({"type": "dahai", "actor": 3, "pai": "5mr", "tsumogiri": false}),
        json!({"type": "tsumo", "actor": 0, "pai": "?"}),
        json!({"type": "dahai", "actor": 0, "pai": "3m", "tsumogiri": false}),
        json!({"type": "tsumo", "actor": 1, "pai": "?"}),
        json!({"type": "reach", "actor": 1}),
        json!({"type": "dahai", "actor": 1, "pai": "2p", "tsumogiri": false}),
        json!({"type": "reach_accepted", "actor": 1}),
        json!({"type": "tsumo", "actor": 2, "pai": "5s"}),
        json!({"type": "dahai", "actor": 2, "pai": "5s", "tsumogiri": true}),
        json!({"type": "hora", "actor": 1, "target": 2, "deltas": [0, 9000, -8000, 0], "ura_markers": ["8p"]}),
        json!({"type": "end_kyoku"}),
    ];
    assert_eq!(ev, expected);
}

/// Regression guard for the seat rotation: mjai actor 0 must be whoever
/// dealt first, which need not be seat 0.
#[test]
fn seats_are_rotated_so_the_first_dealer_is_actor_zero() {
    let mut b = bridge();
    // Second hand of the game (hand_no 1): seat 3 deals, so seat 2 dealt first.
    let ev = feed(
        &mut b,
        &[
            kaiju(1),
            qipai(
                1,
                field(0, 1, 2, 1, "z1"),
                [1, 2, 3, 0],
                [20000, 30000, 26000, 23000],
                MY_DEAL,
            ),
            zimo(3, ""),
            dapai(3, "m1"),
            fulou(1, 2, "m111="),
        ],
    );
    assert_eq!(
        ev[0]["names"],
        json!(["Player2", "Player3", "Player0", "Player1"])
    );
    assert_eq!(ev[0]["id"], json!(3));
    assert_eq!(ev[1]["oya"], json!(1));
    assert_eq!(
        (
            ev[1]["kyoku"].clone(),
            ev[1]["honba"].clone(),
            ev[1]["kyotaku"].clone()
        ),
        (json!(2), json!(2), json!(1))
    );
    assert_eq!(ev[1]["scores"], json!([26000, 23000, 20000, 30000]));
    assert_eq!(ev[1]["tehais"][3][0], json!("2s"));
    assert_eq!(ev[2], json!({"type": "tsumo", "actor": 1, "pai": "?"}));
    // Seat 1 pons from the opposite seat, 3: actors 3 and 1.
    assert_eq!(
        ev[4],
        json!({"type": "pon", "actor": 3, "target": 1, "pai": "1m", "consumed": ["1m", "1m"]})
    );
}

#[test]
fn an_exhaustive_draw_ends_the_hand_with_its_payments() {
    let mut b = bridge();
    feed(
        &mut b,
        &[
            kaiju(0),
            qipai(
                0,
                field(0, 0, 0, 0, "m1"),
                [0, 1, 2, 3],
                [25000; 4],
                MY_DEAL,
            ),
        ],
    );
    let ev = feed(
        &mut b,
        &[push(
            hub::ON_PINGJU,
            json!([
                null,
                1,
                [
                    [null, 0, 1500, "m123", null],
                    [null, 1, 1500, "p456", null],
                    [null, 2, -1500, "", null],
                    [null, 3, -1500, "", null]
                ],
                TIMER
            ]),
        )],
    );
    assert_eq!(
        ev,
        vec![
            json!({"type": "ryukyoku", "deltas": [1500, 1500, -1500, -1500]}),
            json!({"type": "end_kyoku"})
        ]
    );
    // Nothing more until the next deal.
    assert!(feed(&mut b, &[zimo(0, "m1")]).is_empty());
}

#[test]
fn a_double_ron_splits_the_result_per_winner() {
    let mut b = bridge();
    feed(
        &mut b,
        &[
            kaiju(0),
            qipai(
                0,
                field(0, 0, 1, 1, "m1"),
                [0, 1, 2, 3],
                [25000; 4],
                MY_DEAL,
            ),
        ],
    );
    let winner = |seat: u8| {
        json!([
            seat,
            0,
            "m11",
            "m1-",
            30,
            0,
            "1",
            [],
            1000,
            null,
            ["m1"],
            []
        ])
    };
    // Combined: each winner 1000 from seat 0, plus honba 300 and one riichi
    // stick to the first winner.
    let ev = feed(
        &mut b,
        &[push(
            hub::ON_HULE,
            json!([
                null,
                [winner(1), winner(2)],
                [
                    [0, -2300, false, null],
                    [1, 2300, false, null],
                    [2, 1000, false, null],
                    [3, 0, false, null]
                ],
                TIMER
            ]),
        )],
    );
    assert_eq!(
        ev[0],
        json!({"type": "hora", "actor": 1, "target": 0, "deltas": [-1300, 2300, 0, 0]})
    );
    assert_eq!(
        ev[1],
        json!({"type": "hora", "actor": 2, "target": 0, "deltas": [-1000, 0, 1000, 0]})
    );
    assert_eq!(ev[2], json!({"type": "end_kyoku"}));
}

/// Regression: a first deal that failed to parse used to open the game
/// anyway, so the next deal went out without a `start_game`.
#[test]
fn an_unreadable_first_deal_leaves_the_game_unopened() {
    let mut b = bridge();
    feed(&mut b, &[kaiju(0)]);
    // Our own hand hidden (a spectator's view): unreadable for us.
    let hidden = qipai(
        0,
        field(0, 0, 0, 0, "m1"),
        [0, 1, 2, 3],
        [25000; 4],
        &"x1".repeat(13),
    );
    assert!(feed(&mut b, &[hidden]).is_empty());
    let ev = feed(
        &mut b,
        &[qipai(
            0,
            field(0, 0, 0, 0, "m1"),
            [0, 1, 2, 3],
            [25000; 4],
            MY_DEAL,
        )],
    );
    assert_eq!(ev[0]["type"], json!("start_game"));
    assert_eq!(ev[1]["type"], json!("start_kyoku"));
}

#[test]
fn a_deal_with_a_bad_seat_or_duplicate_wind_is_rejected() {
    let mut b = bridge();
    feed(&mut b, &[kaiju(0)]);
    let mut deal = hub::decode(&qipai(
        0,
        field(0, 0, 0, 0, "m1"),
        [0, 1, 2, 3],
        [25000; 4],
        MY_DEAL,
    ))
    .unwrap()
    .body;
    deal[5][3][1] = json!(255);
    assert!(feed(&mut b, &[push(hub::ON_QIPAI, deal)]).is_empty());
    assert!(feed(
        &mut b,
        &[qipai(
            0,
            field(0, 0, 0, 0, "m1"),
            [0, 1, 1, 3],
            [25000; 4],
            MY_DEAL
        )]
    )
    .is_empty());
}

/// A hand that ended through a message we could not read is closed before
/// the next one starts, so consumers never see two open hands.
#[test]
fn a_new_deal_closes_a_hand_left_open() {
    let mut b = bridge();
    feed(
        &mut b,
        &[
            kaiju(0),
            qipai(
                0,
                field(0, 0, 0, 0, "m1"),
                [0, 1, 2, 3],
                [25000; 4],
                MY_DEAL,
            ),
        ],
    );
    assert!(feed(
        &mut b,
        &[push(hub::ON_HULE, json!([null, [[9, 0]], [], TIMER]))]
    )
    .is_empty());
    let ev = feed(
        &mut b,
        &[qipai(
            0,
            field(0, 1, 0, 0, "m1"),
            [3, 0, 1, 2],
            [25000; 4],
            MY_DEAL,
        )],
    );
    assert_eq!(ev[0], json!({"type": "end_kyoku"}));
    assert_eq!(ev[1]["type"], json!("start_kyoku"));
}

#[test]
fn a_tsumo_targets_the_winner() {
    let mut b = bridge();
    feed(
        &mut b,
        &[
            kaiju(0),
            qipai(
                0,
                field(0, 0, 0, 0, "m1"),
                [0, 1, 2, 3],
                [25000; 4],
                MY_DEAL,
            ),
        ],
    );
    let ev = feed(
        &mut b,
        &[push(
            hub::ON_HULE,
            json!([
                null,
                [[
                    3,
                    null,
                    "m11p123",
                    "m1",
                    30,
                    0,
                    "1",
                    [],
                    1100,
                    null,
                    ["m1"],
                    []
                ]],
                [
                    [0, -500, false, null],
                    [1, -300, false, null],
                    [2, -300, false, null],
                    [3, 1100, false, null]
                ],
                TIMER
            ]),
        )],
    );
    assert_eq!(
        ev[0],
        json!({"type": "hora", "actor": 3, "target": 3, "deltas": [-500, -300, -300, 1100]})
    );
}

#[test]
fn a_new_game_closes_one_that_never_reported_a_result() {
    let mut b = bridge();
    feed(
        &mut b,
        &[
            kaiju(2),
            qipai(
                2,
                field(0, 0, 0, 0, "p5"),
                [0, 1, 2, 3],
                [25000; 4],
                MY_DEAL,
            ),
        ],
    );
    let ev = feed(
        &mut b,
        &[
            kaiju(3),
            qipai(
                3,
                field(0, 0, 0, 0, "p5"),
                [0, 1, 2, 3],
                [25000; 4],
                MY_DEAL,
            ),
        ],
    );
    assert_eq!(ev[0], json!({"type": "end_game"}));
    assert_eq!(ev[1]["type"], json!("start_game"));
    assert_eq!(ev[1]["id"], json!(3));
    assert_eq!(ev[2]["type"], json!("start_kyoku"));
}

#[test]
fn play_before_any_deal_is_ignored() {
    let mut b = bridge();
    assert!(feed(&mut b, &[kaiju(0), zimo(0, "m1"), dapai(0, "m1")]).is_empty());
}

#[test]
fn joining_without_the_game_start_takes_our_seat_from_the_deal() {
    let mut b = bridge();
    let ev = feed(
        &mut b,
        &[qipai(
            3,
            field(1, 0, 0, 0, "s1"),
            [0, 1, 2, 3],
            [25000; 4],
            MY_DEAL,
        )],
    );
    assert_eq!(ev[0]["id"], json!(3));
    assert_eq!(ev[1]["bakaze"], json!("S"));
    assert_eq!(ev[1]["tehais"][3][1], json!("P"));
}

#[test]
fn uplink_calls_and_responses_drive_nothing() {
    let mut b = bridge();
    feed(
        &mut b,
        &[
            kaiju(0),
            qipai(
                0,
                field(0, 0, 0, 0, "m1"),
                [0, 1, 2, 3],
                [25000; 4],
                MY_DEAL,
            ),
        ],
    );
    let call = push(hub::SELECT_DAPAI, json!([0, "m1", 0]));
    let r = b.parse(Direction::Up, &call);
    assert!(r.events.is_empty());
    assert_eq!(r.parsed.unwrap().method, "SelectDapai");
    let resp = encode(rmpv::Value::Array(vec![
        1.into(),
        hub::JOIN_ROOM_ASYNC.into(),
        to_mp(&json!([0])),
    ]));
    let r = b.parse(Direction::Down, &resp);
    assert!(r.events.is_empty());
    assert_eq!(r.parsed.unwrap().method, "JoinRoomAsync (response)");
}

#[test]
fn account_bearing_messages_stay_out_of_shareable_logs() {
    let mut b = bridge();
    assert!(!b.parse(Direction::Down, &kaiju(0)).shareable);
    let join = encode(rmpv::Value::Array(vec![
        1.into(),
        hub::JOIN_ROOM_ASYNC.into(),
        to_mp(&json!(["acct-fake-0001", "room", "sess", {}])),
    ]));
    assert!(!b.parse(Direction::Up, &join).shareable);
    assert!(
        !b.parse(Direction::Down, &push(424_242, json!([])))
            .shareable,
        "unknown methods too"
    );
    assert!(b.parse(Direction::Down, &dapai(0, "m1")).shareable);
}

#[test]
fn a_malformed_body_is_skipped_without_events() {
    let mut b = bridge();
    feed(
        &mut b,
        &[
            kaiju(0),
            qipai(
                0,
                field(0, 0, 0, 0, "m1"),
                [0, 1, 2, 3],
                [25000; 4],
                MY_DEAL,
            ),
        ],
    );
    assert!(feed(
        &mut b,
        &[
            push(hub::ON_DAPAI, json!([null, 0, "q9"])),
            push(hub::ON_FULOU, json!([null, 9, 1, "m1-23"]))
        ]
    )
    .is_empty());
    assert!(b.parse(Direction::Down, b"\x01\x02\x03").events.is_empty());
}

#[test]
fn draw_payments_are_keyed_by_wind() {
    let mut b = bridge();
    // Seat 1 deals (East), so seat 2 is South (wind 1) and seat 0 is North.
    feed(
        &mut b,
        &[
            kaiju(0),
            qipai(
                0,
                field(0, 1, 0, 0, "m1"),
                [3, 0, 1, 2],
                [25000; 4],
                MY_DEAL,
            ),
        ],
    );
    let ev = feed(
        &mut b,
        &[push(
            hub::ON_PINGJU,
            json!([
                null,
                1,
                [
                    [null, 0, -1000, "", null],
                    [null, 1, 3000, "s2344,p78-9,z555=,m111+", null],
                    [null, 2, -1000, "", null],
                    [null, 3, -1000, "", null]
                ],
                TIMER
            ]),
        )],
    );
    // First dealer was seat 0, so actors are seats here; seat 2 (South) was tenpai.
    assert_eq!(
        ev[0],
        json!({"type": "ryukyoku", "deltas": [-1000, -1000, 3000, -1000]})
    );
}

#[test]
fn kyotaku_counts_every_stick_denomination() {
    let mut b = bridge();
    let mut f = field(0, 0, 3, 0, "m1");
    f[3] = json!([3, 0, 2, 0, 0]); // two riichi sticks plus three 100-point honba sticks
    let ev = feed(
        &mut b,
        &[kaiju(0), qipai(0, f, [0, 1, 2, 3], [25000; 4], MY_DEAL)],
    );
    assert_eq!(ev[1]["kyotaku"], json!(2));
}

#[test]
fn kans_and_their_new_dora() {
    let mut b = bridge();
    feed(
        &mut b,
        &[
            kaiju(0),
            qipai(
                0,
                field(0, 0, 0, 0, "m1"),
                [0, 1, 2, 3],
                [25000; 4],
                MY_DEAL,
            ),
        ],
    );
    let kan = |m: &str, kind: u8| {
        push(
            hub::ON_AN_GANG_ADD_GANG,
            json!([null, 0, kind, m, null, TIMER]),
        )
    };
    let ev = feed(
        &mut b,
        &[
            dapai(1, "p9"),
            fulou(0, 3, "p9999+"),
            zimo(0, "s1"),
            dapai(0, "s1_"),
            push(hub::ON_ADD_BAOPAI, json!([null, "p2"])),
            kan("z1111", 4),
            push(hub::ON_ADD_BAOPAI, json!([null, "m5"])),
            kan("m555=0", 5),
        ],
    );
    assert_eq!(
        ev[1],
        json!({"type": "daiminkan", "actor": 0, "target": 1, "pai": "9p", "consumed": ["9p", "9p", "9p"]})
    );
    assert_eq!(ev[4], json!({"type": "dora", "dora_marker": "2p"}));
    assert_eq!(
        ev[5],
        json!({"type": "ankan", "actor": 0, "consumed": ["E", "E", "E", "E"]})
    );
    assert_eq!(ev[6], json!({"type": "dora", "dora_marker": "5m"}));
    assert_eq!(
        ev[7],
        json!({"type": "kakan", "actor": 0, "pai": "5mr", "consumed": ["5m", "5m", "5m"]})
    );
}

#[test]
fn the_game_result_closes_the_game_with_ranks_by_actor() {
    let mut b = bridge();
    // Seat 1 dealt first, so actor 0 is seat 1.
    feed(
        &mut b,
        &[
            kaiju(0),
            qipai(
                0,
                field(0, 0, 0, 0, "m1"),
                [3, 0, 1, 2],
                [25000; 4],
                MY_DEAL,
            ),
        ],
    );
    let player =
        |seat: u8, rank: u8, score: i32| json!([null, seat, rank, score, "0.0", null, null, null]);
    let r = b.parse(
        Direction::Down,
        &push(
            hub::ON_JIEJU,
            json!([
                null,
                [
                    player(3, 2, 30700),
                    player(0, 4, 11100),
                    player(1, 1, 33800),
                    player(2, 3, 24400)
                ],
                [0, 0, 0, 0],
                [[["acct-fake-0001"]]]
            ]),
        ),
    );
    assert!(!r.shareable);
    match &r.events[..] {
        [MjaiEvent::EndGame {
            reason: GameEndReason::Confirmed,
            final_scores,
            final_ranks,
        }] => {
            assert_eq!(
                final_scores.as_deref(),
                Some(&[33800, 24400, 30700, 11100][..])
            );
            assert_eq!(final_ranks.as_deref(), Some(&[1, 3, 2, 4][..]));
        }
        other => panic!("{other:?}"),
    }
    // Closed: the next game's start does not emit a second end_game.
    let ev = feed(&mut b, &[kaiju(0)]);
    assert!(ev.is_empty());
}

#[test]
fn a_reconnect_prelude_restores_seat_and_names() {
    let mut b = bridge();
    let k = hub::decode(&kaiju(2)).unwrap().body;
    assert!(feed(&mut b, &[push(hub::ON_ZHUNBEI_CHONGLIAN, json!([null, k]))]).is_empty());
    let ev = feed(
        &mut b,
        &[qipai(
            2,
            field(0, 0, 0, 0, "m1"),
            [0, 1, 2, 3],
            [25000; 4],
            MY_DEAL,
        )],
    );
    assert_eq!(ev[0]["names"][3], json!("Player3"));
    assert_eq!(ev[0]["id"], json!(2));
}

#[test]
fn a_deal_naming_an_account_is_withheld() {
    let mut b = bridge();
    let mut deal = hub::decode(&qipai(
        0,
        field(0, 0, 0, 0, "m1"),
        [0, 1, 2, 3],
        [25000; 4],
        MY_DEAL,
    ))
    .unwrap()
    .body;
    assert!(
        b.parse(Direction::Down, &push(hub::ON_QIPAI, deal.clone()))
            .shareable
    );
    deal[5][0][0] = json!("acct-fake-0001");
    assert!(
        !b.parse(Direction::Down, &push(hub::ON_QIPAI, deal))
            .shareable
    );
}

#[test]
fn an_agreed_end_vote_ends_the_game() {
    let mut b = bridge();
    feed(
        &mut b,
        &[
            kaiju(2),
            qipai(
                2,
                field(0, 0, 0, 0, "p5"),
                [0, 1, 2, 3],
                [25000; 4],
                MY_DEAL,
            ),
        ],
    );
    // Still counting votes: nothing yet.
    assert!(feed(
        &mut b,
        &[push(
            hub::ON_UPDATE_END_GAME_VOTE,
            json!([null, null, [0, null, null, null]])
        )]
    )
    .is_empty());
    let r = b.parse(
        Direction::Down,
        &push(hub::ON_UPDATE_END_GAME_VOTE, json!([null, 0, [0, 0, 0, 0]])),
    );
    assert!(matches!(
        &r.events[..],
        [MjaiEvent::EndGame {
            reason: GameEndReason::Terminated,
            ..
        }]
    ));
    assert!(
        feed(&mut b, &[zimo(2, "m1")]).is_empty(),
        "play after the vote is ignored"
    );
}

#[test]
fn a_rejected_end_vote_changes_nothing() {
    let mut b = bridge();
    feed(
        &mut b,
        &[
            kaiju(2),
            qipai(
                2,
                field(0, 0, 0, 0, "p5"),
                [0, 1, 2, 3],
                [25000; 4],
                MY_DEAL,
            ),
        ],
    );
    assert!(feed(
        &mut b,
        &[push(
            hub::ON_UPDATE_END_GAME_VOTE,
            json!([null, 1, [0, 1, 0, 0]])
        )]
    )
    .is_empty());
    assert_eq!(feed(&mut b, &[zimo(2, "m1")]).len(), 1);
}

fn kaiju_seats(my_seat: u8, seats: &[u8]) -> Vec<u8> {
    let players: Vec<JsonValue> = seats
        .iter()
        .map(|&seat| {
            let me = seat == my_seat;
            json!([0, [seat, null, ["", format!("Player{seat}"), 1], true, !me]])
        })
        .collect();
    push(
        hub::ON_KAIJU,
        json!([
            null,
            [10000, null],
            my_seat,
            [null, seats.len(), 35000, 40000],
            TIMER,
            players,
            {},
            [0, 0, 0, [0, 0, 0, 0, 0], null, 108]
        ]),
    )
}

/// `players` is `(seat, wind, score)`; our hand goes to `me`.
fn qipai_seats(me: u8, field: JsonValue, players: &[(u8, u8, i32)], my_hand: &str) -> Vec<u8> {
    let players: Vec<JsonValue> = players
        .iter()
        .map(|&(seat, wind, score)| {
            let hand = if seat == me {
                my_hand.to_string()
            } else {
                "x1".repeat(13)
            };
            json!([null, seat, wind, score, null, hand, [], 0, 0, []])
        })
        .collect();
    push(
        hub::ON_QIPAI,
        json!([null, me, 1_700_000_000_000i64, null, field, players, TIMER]),
    )
}

#[test]
fn a_three_player_hand_with_kita_a_pon_and_a_draw() {
    let mut b = bridge();
    let ev = feed(
        &mut b,
        &[
            kaiju_seats(1, &[0, 1, 2]),
            qipai_seats(
                1,
                field(0, 0, 0, 0, "m1"),
                &[(0, 0, 35000), (1, 1, 35000), (2, 2, 35000)],
                MY_DEAL,
            ),
            zimo(0, ""),
            push(hub::ON_BABEI, json!([null, 0, "z4", null, TIMER])),
            zimo(0, ""),
            dapai(0, "p1"),
            // The mark is not read: the pon takes seat 0's discard.
            fulou(2, 2, "p111="),
            dapai(2, "s9"),
            push(
                hub::ON_PINGJU,
                json!([
                    null,
                    1,
                    [
                        [null, 0, -1000, "", null],
                        [null, 1, 2000, "p123", null],
                        [null, 2, -1000, "", null]
                    ],
                    TIMER
                ]),
            ),
        ],
    );
    assert_eq!(
        ev[0],
        json!({"type": "start_game", "names": ["Player0", "Player1", "Player2"], "id": 1, "num_players": 3})
    );
    assert_eq!(ev[1]["num_players"], json!(3));
    assert_eq!(ev[1]["scores"], json!([35000, 35000, 35000]));
    assert_eq!(ev[1]["tehais"].as_array().unwrap().len(), 3);
    assert_eq!(ev[1]["tehais"][1][0], json!("2s"));
    assert_eq!(ev[3], json!({"type": "kita", "actor": 0, "pai": "N"}));
    assert_eq!(
        ev[6],
        json!({"type": "pon", "actor": 2, "target": 0, "pai": "1p", "consumed": ["1p", "1p"]})
    );
    assert_eq!(
        ev[8],
        json!({"type": "ryukyoku", "deltas": [-1000, 2000, -1000]})
    );
    assert_eq!(ev[9], json!({"type": "end_kyoku"}));
}

/// Seat numbers need not be 0..n: actors come from the winds, so a
/// 3-player table numbered like a 4-seat one with a gap still maps right.
#[test]
fn three_player_seats_follow_winds_whatever_their_numbers() {
    let mut b = bridge();
    // Second hand: seat 1 deals, then seat 3, then seat 0; seat 0 dealt first.
    let ev = feed(
        &mut b,
        &[
            qipai_seats(
                3,
                field(0, 1, 0, 0, "m1"),
                &[(0, 2, 30000), (1, 0, 40000), (3, 1, 35000)],
                MY_DEAL,
            ),
            zimo(3, ""),
            dapai(3, "m9"),
            fulou(0, 2, "m999-"),
        ],
    );
    assert_eq!(ev[0]["num_players"], json!(3));
    assert_eq!(ev[0]["id"], json!(2));
    assert_eq!(ev[1]["oya"], json!(1));
    assert_eq!(ev[1]["scores"], json!([30000, 40000, 35000]));
    assert_eq!(ev[2], json!({"type": "tsumo", "actor": 2, "pai": "?"}));
    assert_eq!(
        ev[4],
        json!({"type": "pon", "actor": 0, "target": 2, "pai": "9m", "consumed": ["9m", "9m"]})
    );
}

#[test]
fn a_three_player_result_ranks_three_actors() {
    let mut b = bridge();
    feed(
        &mut b,
        &[
            kaiju_seats(0, &[0, 1, 2]),
            qipai_seats(
                0,
                field(0, 0, 0, 0, "m1"),
                &[(0, 0, 35000), (1, 1, 35000), (2, 2, 35000)],
                MY_DEAL,
            ),
        ],
    );
    let player =
        |seat: u8, rank: u8, score: i32| json!([null, seat, rank, score, "0.0", null, null, null]);
    let r = b.parse(
        Direction::Down,
        &push(
            hub::ON_JIEJU,
            json!([
                null,
                [
                    player(0, 2, 36000),
                    player(1, 1, 50000),
                    player(2, 3, 19000)
                ],
                [0, 0, 0, 0],
                []
            ]),
        ),
    );
    match &r.events[..] {
        [MjaiEvent::EndGame {
            final_scores,
            final_ranks,
            ..
        }] => {
            assert_eq!(final_scores.as_deref(), Some(&[36000, 50000, 19000][..]));
            assert_eq!(final_ranks.as_deref(), Some(&[2, 1, 3][..]));
        }
        other => panic!("{other:?}"),
    }
}

/// The call's target is whoever made the discard, even where the direction
/// mark would say otherwise.
#[test]
fn a_call_targets_the_last_discarder() {
    let mut b = bridge();
    feed(
        &mut b,
        &[
            kaiju(0),
            qipai(
                0,
                field(0, 0, 0, 0, "m1"),
                [0, 1, 2, 3],
                [25000; 4],
                MY_DEAL,
            ),
        ],
    );
    let ev = feed(&mut b, &[dapai(3, "m1"), fulou(1, 2, "m111+")]);
    assert_eq!(ev[1]["target"], json!(3));
}

/// Regression: the wire marks a kita of the tile just drawn like a
/// tsumogiri (`"z4_"`); reading it as a bare tile dropped every such kita.
#[test]
fn a_kita_of_the_drawn_north_carries_the_tsumogiri_mark() {
    let mut b = bridge();
    feed(
        &mut b,
        &[
            kaiju_seats(0, &[0, 1, 2]),
            qipai_seats(
                0,
                field(0, 0, 0, 0, "m1"),
                &[(0, 0, 35000), (1, 1, 35000), (2, 2, 35000)],
                MY_DEAL,
            ),
        ],
    );
    let no_hule = json!([0, null, null, null, null, 0, null, null, 0, null, null, null]);
    let ev = feed(
        &mut b,
        &[
            push(
                hub::ON_ZIMO,
                json!([null, 0, "z4", [], ["z4_"], no_hule, TIMER]),
            ),
            push(hub::ON_BABEI, json!([null, 0, "z4_", no_hule, TIMER])),
            push(hub::ON_ZIMO, json!([null, 0, "p5", [], [], no_hule, TIMER])),
        ],
    );
    assert_eq!(
        ev,
        vec![
            json!({"type": "tsumo", "actor": 0, "pai": "N"}),
            json!({"type": "kita", "actor": 0, "pai": "N"}),
            json!({"type": "tsumo", "actor": 0, "pai": "5p"}),
        ]
    );
}

/// Regression guard from a live 3-player game: the mark counts 4-seat
/// geometry, so seat 2 calling seat 0 (its next player) is marked `=`.
#[test]
fn a_three_player_mark_counts_four_seats() {
    let mut b = bridge();
    feed(
        &mut b,
        &[
            kaiju_seats(1, &[0, 1, 2]),
            qipai_seats(
                1,
                field(0, 0, 0, 0, "m1"),
                &[(0, 0, 35000), (1, 1, 35000), (2, 2, 35000)],
                MY_DEAL,
            ),
        ],
    );
    // No discard seen first, so only the mark can name the target.
    let ev = feed(
        &mut b,
        &[fulou(2, 2, "z777="), zimo(0, ""), fulou(1, 2, "z555-")],
    );
    assert_eq!(ev[0]["target"], json!(0));
    assert_eq!(ev[2]["target"], json!(0));
}

#[test]
fn an_open_riichi_discard_declares_riichi() {
    let mut b = bridge();
    feed(
        &mut b,
        &[
            kaiju(0),
            qipai(
                0,
                field(0, 0, 0, 0, "m1"),
                [0, 1, 2, 3],
                [25000; 4],
                MY_DEAL,
            ),
        ],
    );
    let ev = feed(
        &mut b,
        &[
            zimo(2, ""),
            push(
                hub::ON_DAPAI,
                json!([
                    null,
                    2,
                    "s7_@",
                    [],
                    [],
                    [],
                    [0],
                    "",
                    3,
                    "m123p456s789z11",
                    TIMER
                ]),
            ),
        ],
    );
    assert_eq!(ev[1], json!({"type": "reach", "actor": 2}));
    assert_eq!(
        ev[2],
        json!({"type": "dahai", "actor": 2, "pai": "7s", "tsumogiri": true})
    );
}

#[test]
fn the_server_terminating_the_battle_ends_the_game() {
    let mut b = bridge();
    feed(
        &mut b,
        &[
            kaiju(0),
            qipai(
                0,
                field(0, 0, 0, 0, "m1"),
                [0, 1, 2, 3],
                [25000; 4],
                MY_DEAL,
            ),
        ],
    );
    assert!(feed(
        &mut b,
        &[push(hub::ON_SYSTEM_CALL_EVENT, json!([2, "pause"]))]
    )
    .is_empty());
    let r = b.parse(
        Direction::Down,
        &push(hub::ON_SYSTEM_CALL_EVENT, json!([1, "terminated"])),
    );
    assert!(matches!(
        &r.events[..],
        [MjaiEvent::EndGame {
            reason: GameEndReason::Terminated,
            ..
        }]
    ));
    assert!(feed(&mut b, &[zimo(0, "m1")]).is_empty());
}

/// `players` is `(seat, wind, score, hand, river, riichi_type, kita)`.
#[allow(clippy::type_complexity)]
fn snapshot(
    our_wind: u8,
    field: JsonValue,
    players: &[(u8, u8, i32, &str, &[&str], u8, &[&str])],
) -> Vec<u8> {
    let players: Vec<JsonValue> = players
        .iter()
        .map(|&(seat, wind, score, hand, river, riichi, kita)| {
            json!([null, seat, wind, score, null, hand, river, 0, riichi, kita])
        })
        .collect();
    push(
        hub::ON_CHONGXIN_LIANJIE_CHENGGONG,
        json!([
            null,
            our_wind,
            1_700_000_000_000i64,
            field,
            players,
            null,
            null
        ]),
    )
}

/// A hand with a pon, our chi, a riichi, an open kan on the riichi player's
/// tsumogiri and our closed kan, cut after our discard.
fn reconnect_4p() -> Vec<u8> {
    let field = json!([
        0,
        0,
        0,
        [0, 0, 1, 0, 0],
        ["m4", "p6", "s3"],
        50,
        null,
        "h",
        "s",
        false
    ]);
    let x = |n: usize| "x1".repeat(n);
    snapshot(
        2,
        field,
        &[
            (0, 0, 24000, &(x(13) + "*"), &["m1", "s1*", "z1_-"], 1, &[]),
            (
                1,
                1,
                25000,
                &(x(10) + ",z1111-"),
                &["z5=", "m3-", "m9"],
                0,
                &[],
            ),
            (2, 2, 25000, "m11s234z66,m3-45,p2222", &["p9", "s7"], 0, &[]),
            (3, 3, 25000, &(x(10) + ",z555="), &["s9", "p1_"], 0, &[]),
        ],
    )
}

#[test]
fn a_reconnect_rebuilds_the_hand_in_progress() {
    let mut b = bridge();
    let k = hub::decode(&kaiju(2)).unwrap().body;
    feed(&mut b, &[push(hub::ON_ZHUNBEI_CHONGLIAN, json!([null, k]))]);
    let ev = feed(&mut b, &[reconnect_4p()]);
    assert_eq!(ev[0]["type"], json!("start_game"));
    assert_eq!(ev[0]["id"], json!(2));
    let start = &ev[1];
    // The riichi stick goes back: start-of-hand scores, no sticks yet.
    assert_eq!(start["scores"], json!([25000, 25000, 25000, 25000]));
    assert_eq!(start["kyotaku"], json!(0));
    assert_eq!(start["dora_marker"], json!("4m"));
    let mut opening: Vec<String> = serde_json::from_value(start["tehais"][2].clone()).unwrap();
    opening.sort();
    assert_eq!(
        opening,
        ["1m", "1m", "2p", "2p", "2p", "2s", "3s", "4m", "4s", "5m", "9p", "F", "F"]
    );
    let history: Vec<JsonValue> = ev[2..].to_vec();
    // Not our turn at the cut: the whole history is catch-up.
    assert_eq!(
        start["catch_up"],
        JsonValue::Null,
        "catch_up is not serialized"
    );
    let expected = vec![
        json!({"type": "tsumo", "actor": 0, "pai": "?"}),
        json!({"type": "dahai", "actor": 0, "pai": "1m", "tsumogiri": false}),
        json!({"type": "tsumo", "actor": 1, "pai": "?"}),
        json!({"type": "dahai", "actor": 1, "pai": "P", "tsumogiri": false}),
        json!({"type": "pon", "actor": 3, "target": 1, "pai": "P", "consumed": ["P", "P"]}),
        json!({"type": "dahai", "actor": 3, "pai": "9s", "tsumogiri": false}),
        json!({"type": "tsumo", "actor": 0, "pai": "?"}),
        json!({"type": "reach", "actor": 0}),
        json!({"type": "dahai", "actor": 0, "pai": "1s", "tsumogiri": false}),
        json!({"type": "reach_accepted", "actor": 0}),
        json!({"type": "tsumo", "actor": 1, "pai": "?"}),
        json!({"type": "dahai", "actor": 1, "pai": "3m", "tsumogiri": false}),
        json!({"type": "chi", "actor": 2, "target": 1, "pai": "3m", "consumed": ["4m", "5m"]}),
        json!({"type": "dahai", "actor": 2, "pai": "9p", "tsumogiri": false}),
        json!({"type": "tsumo", "actor": 3, "pai": "?"}),
        json!({"type": "dahai", "actor": 3, "pai": "1p", "tsumogiri": true}),
        json!({"type": "tsumo", "actor": 0, "pai": "?"}),
        json!({"type": "dahai", "actor": 0, "pai": "E", "tsumogiri": true}),
        json!({"type": "daiminkan", "actor": 1, "target": 0, "pai": "E", "consumed": ["E", "E", "E"]}),
        json!({"type": "tsumo", "actor": 1, "pai": "?"}),
        json!({"type": "dora", "dora_marker": "6p"}),
        json!({"type": "dahai", "actor": 1, "pai": "9m", "tsumogiri": false}),
        json!({"type": "tsumo", "actor": 2, "pai": "2p"}),
        json!({"type": "ankan", "actor": 2, "consumed": ["2p", "2p", "2p", "2p"]}),
        json!({"type": "dora", "dora_marker": "3s"}),
        json!({"type": "tsumo", "actor": 2, "pai": "7s"}),
        json!({"type": "dahai", "actor": 2, "pai": "7s", "tsumogiri": false}),
    ];
    assert_eq!(history, expected);
    // Live play picks up: a pon of our last discard targets us.
    let ev = feed(&mut b, &[fulou(3, 2, "s777=")]);
    assert_eq!(ev[0]["target"], json!(2));
}

#[test]
fn a_reconnect_on_our_draw_leaves_that_draw_to_the_bot() {
    let mut b = bridge();
    let field = json!([0, 1, 0, [0, 0, 0, 0, 0], ["s1"], 40, null, "h", "s", false]);
    let ev = feed(
        &mut b,
        &[snapshot(
            2,
            field,
            &[
                (0, 1, 35000, &"x1".repeat(13), &["m1", "p9"], 0, &[]),
                (1, 2, 35000, "m1p24468s1346z255p0", &["s9_"], 0, &[]),
                (
                    2,
                    0,
                    35000,
                    &("x1".repeat(13) + ",z4"),
                    &["z6", "z7"],
                    0,
                    &["z4"],
                ),
            ],
        )],
    );
    let history = &ev[2..];
    // Dealer seat 2 sets a North aside in its first turn.
    assert_eq!(history[0], json!({"type": "tsumo", "actor": 1, "pai": "?"}));
    assert_eq!(history[1], json!({"type": "kita", "actor": 1, "pai": "N"}));
    assert_eq!(
        history.last().unwrap(),
        &json!({"type": "tsumo", "actor": 0, "pai": "5pr"})
    );
    match &b
        .parse(
            Direction::Down,
            &snapshot(1, json!([0, 1, 0, [0, 0, 0, 0, 0], ["s1"], 40]), &[]),
        )
        .events[..]
    {
        [] => {}
        other => panic!("a second snapshot mid-hand is ignored: {other:?}"),
    }
}

#[test]
fn the_rebuilt_catch_up_count_leaves_our_pending_draw_out() {
    let mut b = bridge();
    let field = json!([0, 1, 0, [0, 0, 0, 0, 0], ["s1"], 40, null, "h", "s", false]);
    let r = b.parse(
        Direction::Down,
        &snapshot(
            2,
            field,
            &[
                (0, 1, 35000, &"x1".repeat(13), &["m1", "p9"], 0, &[]),
                (1, 2, 35000, "m1p24468s1346z255p0", &["s9_"], 0, &[]),
                (2, 0, 35000, &"x1".repeat(13), &["z6", "z7"], 0, &[]),
            ],
        ),
    );
    let MjaiEvent::StartKyoku { catch_up, .. } = &r.events[1] else {
        panic!()
    };
    let after = r.events.len() - 2;
    assert_eq!(*catch_up, after - 1);
}

#[test]
fn a_snapshot_no_history_fits_resumes_at_the_next_deal() {
    let mut b = bridge();
    feed(&mut b, &[kaiju(2)]);
    let field = json!([0, 0, 0, [0, 0, 0, 0, 0], ["m4"], 50, null, "h", "s", false]);
    // Seat 1's river says seat 2 chied it, but seat 2 shows no chi.
    let bad = snapshot(
        2,
        field,
        &[
            (0, 0, 25000, &"x1".repeat(13), &["m1"], 0, &[]),
            (1, 1, 25000, &"x1".repeat(13), &["m3-"], 0, &[]),
            (2, 2, 25000, "m11s234z66p22229", &[], 0, &[]),
            (3, 3, 25000, &"x1".repeat(13), &[], 0, &[]),
        ],
    );
    assert!(feed(&mut b, &[bad, zimo(3, "")]).is_empty());
    let ev = feed(
        &mut b,
        &[qipai(2, field_next(), [0, 1, 2, 3], [25000; 4], MY_DEAL)],
    );
    assert_eq!(ev[0]["type"], json!("start_game"));
}

fn field_next() -> JsonValue {
    field(0, 1, 0, 0, "m1")
}
