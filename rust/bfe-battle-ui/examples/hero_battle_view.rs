//! Build the local Hero Assault screen from plain fixed-tick observations.
//! Names, clocks and positions in this example are invented.
use bfe_battle_ui::battle2::*;

fn main() {
    let map = Caption::Key("world.reeds");
    let teams = [
        (3, "team.west", "leaf", 14, true),
        (6, "team.east", "seed", 12, false),
    ]
    .map(|(id, key, icon, score, show_score)| Team {
        id,
        name: Caption::Key(key),
        icon,
        score,
        target: Some(42),
        reinforcements: Some(17),
        show_score,
    });
    let subject = Subject {
        id: 18,
        generation: 4,
    };
    // At the next completed tick, copy numbers before releasing the world borrow.
    let previous = InfantrySample {
        epoch: 5,
        tick: 201,
        subject,
        slab: 2,
        health: 61.,
        stamina: 9.,
        map: [0.32, 0.46],
        yaw: 3.12,
    };
    let now = InfantrySample {
        tick: 202,
        health: 58.,
        stamina: -1.,
        map: [0.32, 0.45],
        yaw: -3.13,
        ..previous
    };
    let primary = Slot {
        kind: SlotKind::Weapon,
        name: Caption::Key("gear.lantern"),
        icon: "lantern",
        changed_at: Some(194),
        rounds: Some(7),
        reserve: Some(19),
        charge: None,
        ready_at: None,
        clip: None,
        heat: None,
        overheated: false,
        refire: None,
    };
    let secondary = Slot {
        kind: SlotKind::Power,
        name: Caption::Key("gear.gust"),
        icon: "gust",
        rounds: None,
        reserve: None,
        ..primary
    };
    let body = Body::from_infantry(
        Some(previous),
        now,
        73.,
        24.,
        Role::RosterHero,
        [Some(primary), Some(secondary)],
        "dot",
    );

    // These arrays stand in for the adapter's retained buffers. It updates them once per tick.
    let contacts = [Contact {
        subject: Subject {
            id: 27,
            generation: 2,
        },
        team: 6,
        icon: "mark",
        map: [
            Sample {
                previous: 0.39,
                current: 0.40,
            },
            Sample::steady(0.47),
        ],
    }];
    let objectives = [Objective {
        id: 2,
        audience: Audience::Team(3),
        text: Caption::Key("order.hold"),
        complete: false,
    }];
    let timers = [Timer {
        name: Caption::Key("clock.signal"),
        audience: Audience::All,
        due: 684,
    }];
    let events = [
        Event {
            sequence: 29,
            until_tick: 248,
            audience: Audience::Team(3),
            message: Message::Notice(Caption::Key("notice.dock")),
        },
        Event {
            sequence: 30,
            until_tick: 251,
            audience: Audience::All,
            message: Message::Kill {
                killer: "Pine",
                victim: "Fern",
                weapon: "gear.lantern",
                weapon_name: Some("Lantern"),
            },
        },
    ];
    let scores = [ScoreRow {
        player: 7,
        team: 3,
        name: "Pine",
        score: 11,
        kills: 3,
        deaths: 2,
        assists: 0,
        ping: None,
    }];
    let observe = |phase, body| BattleView {
        body,
        contacts: &contacts,
        objectives: &objectives,
        timers: &timers,
        events: &events,
        scores: &scores,
        leaving: false,
        ..BattleView::for_hero_assault(now.tick, phase, (7, 3), map, "reeds_map", teams)
    };

    // The spawn launcher, shared with the 2004 game, draws while the player chooses.
    // Absence of a body alone also occurs during death. A committed result takes
    // precedence over a surviving body.
    let deploy = observe(Phase::Spawn, None);
    let playing = observe(Phase::Playing, Some(body));
    let result = observe(Phase::Ended(Outcome::Winner(3)), Some(body));
    assert!(deploy.body.is_none());
    assert_eq!(playing.body.unwrap().health.value.at(0.5), 59.5);
    assert_eq!(result.scores[0].score, 11);
}
