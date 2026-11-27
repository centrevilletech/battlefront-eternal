use super::*;
use crate::OwnedUiSurface;
use bfe_render::hud_model2::{catalog::Catalog, Layout};

fn team(id: u8) -> Team<'static> {
    Team {
        id,
        name: Caption::Literal("Cedar"),
        icon: "cedar",
        score: 12,
        target: Some(24),
        reinforcements: None,
        show_score: true,
    }
}

pub(super) fn view() -> BattleView<'static> {
    BattleView {
        tick: 9,
        phase: Phase::Playing,
        mode: Mode::HeroAssault,
        player: 4,
        team: 7,
        map_name: Caption::Literal("Marsh"),
        map_image: "paper",
        teams: [team(7), team(12)],
        body: None,
        vehicle: None,
        aim: Aim::default(),
        target: None,
        contacts: &[],
        systems: &[],
        objectives: &[],
        timers: &[],
        events: &[],
        scores: &[],
        leaving: false,
    }
}
fn draws(s: &mut Screens, v: &BattleView<'_>, f: f32) -> (Vec<f32>, Vec<f32>) {
    let mut surface = OwnedUiSurface::default();
    s.draw(&mut surface, v, [1024., 768.], f);
    let n = surface.batch.count;
    (
        surface.batch.position[..n * 12].to_vec(),
        surface.batch.tint[..n * 16].to_vec(),
    )
}

#[test]
fn targeted_messages_expire_on_the_tick_and_drawing_does_not_consume_them() {
    let mut v = view();
    let layout=Layout::read(&br#"{"file":"1playerhud","entries":[{"name":"MultilineText","values":["feed"],"children":[{"name":"Rect","values":[400,80]},{"name":"EventText","values":["player1.message"]}]}]}"#[..],
        &br#"{"file":"hudtransforms","entries":[]}"#[..]).unwrap();
    let mut catalog = Catalog::default();
    catalog.layout = layout;
    let mut screens = Screens::new(catalog, "bf1");
    let initial = draws(&mut screens, &v, 0.);
    let message = Event {
        sequence: 3,
        until_tick: 10,
        audience: Audience::Team(12),
        message: Message::Notice(Caption::Literal("Rain")),
    };
    v.events = std::slice::from_ref(&message);
    assert_eq!(initial, draws(&mut screens, &v, 0.5));
    let visible = Event {
        audience: Audience::Player(4),
        ..message
    };
    v.events = std::slice::from_ref(&visible);
    let with = draws(&mut screens, &v, 0.5);
    assert_ne!(initial, with);
    assert_eq!(with, draws(&mut screens, &v, 0.9));
    v.tick = 10;
    assert_eq!(initial, draws(&mut screens, &v, 0.));
}

#[test]
fn the_spawn_phase_leaves_the_screen_to_the_launcher_and_a_roster_release_returns_to_play() {
    let mut screens = Screens::new(Catalog::default(), "eternal");
    let mut v = view();
    v.phase = Phase::Spawn;
    assert_eq!(draws(&mut screens, &v, 0.5).0.len(), 0);
    assert_eq!(screens.hit([448. + 55., 196.]), Hit::None);
    v.phase = Phase::Playing;
    let play = draws(&mut screens, &v, 0.);
    screens.input(&v, Action::Roster(true));
    assert_ne!(draws(&mut screens, &v, 0.), play);
    screens.input(&v, Action::Roster(false));
    assert_eq!(draws(&mut screens, &v, 0.), play);
    v.phase = Phase::Ended(Outcome::Draw);
    assert_eq!(screens.input(&v, Action::Confirm), Response::Done);
}

#[test]
fn a_landing_hint_draws_the_current_pilot_key_and_keeps_its_unbound_action() {
    let mut catalog = Catalog::default();
    catalog.layout = Layout::read(
        br#"{"file":"1playerhud","entries":[{"name":"text","values":["hint"],"children":[
            {"name":"position","values":[200,600]},{"name":"TextBox","values":[0.4,0.05]},
            {"name":"EventText","values":["player1.tooltips"]}]}]}"#
            .as_slice(),
        br#"{"file":"hudtransforms","entries":[]}"#.as_slice(),
    )
    .unwrap();
    let mut screens = Screens::new(catalog, "bf1");
    let mut surface = OwnedUiSurface::default();
    for (ch, page) in [(b'Q', 912), (b'J', 913), (b'Z', 914)] {
        surface.fonts[1]
            .glyphs
            .get_mut(&u16::from(ch))
            .unwrap()
            .page = page;
    }
    let pilot = Vehicle {
        subject: Subject::default(),
        seat: Seat::Pilot,
        map_pose: None,
        hull: Gauge::default(),
        speed: Gauge::default(),
        boost: None,
        slots: [None; 2],
        landing: Some(Caption::Literal("Zone")),
    };
    for (seat, key, expected) in [
        (Seat::Pilot, "Q", [true, false, true]),
        (Seat::Pilot, "J", [false, true, true]),
        (Seat::Pilot, "", [false, false, true]),
        (Seat::Gunner, "J", [false, false, false]),
    ] {
        let v = BattleView {
            vehicle: Some(Vehicle { seat, ..pilot }),
            ..view()
        };
        screens.takeoff_key = key.to_owned();
        surface.batch.begin();
        screens.draw(&mut surface, &v, [1024., 768.], 1.);
        assert_eq!(
            [912, 913, 914].map(|id| surface.batch.draws.iter().any(|d| d.texture == id)),
            expected
        );
    }
}

#[test]
fn long_player_names_and_full_teams_leave_room_for_the_scoreboard_controls() {
    let row = ScoreRow {
        team: 7,
        player: 10,
        name: "illillillillillillillillillillii",
        score: 64052,
        kills: 126,
        deaths: 73,
        assists: 5,
        ping: None,
    };
    let rows: Vec<_> = (0..64)
        .map(|i| ScoreRow {
            player: i,
            team: if i % 2 == 0 { 7 } else { 12 },
            ..row
        })
        .collect();
    let mut v = view();
    v.phase = Phase::Ended(Outcome::Draw);
    v.scores = &rows;
    let mut screens = Screens::new(Catalog::default(), "bf2");
    let mut surface = OwnedUiSurface::default();
    screens.draw(&mut surface, &v, [1280., 720.], 0.);
    assert!(surface.batch.count < bfe_render::interface::QUADS);
    assert_eq!(
        screens.hit([160. + 793. * 0.9375, 690. * 0.9375]),
        Hit::Done
    );
    for _ in 0..40 {
        screens.input(&v, Action::Next);
    }
    surface.batch.begin();
    screens.draw(&mut surface, &v, [1280., 720.], 0.);
    assert!(surface.batch.count < bfe_render::interface::QUADS);
}

#[test]
fn interpolated_compass_crosses_the_heading_seam_by_the_short_turn() {
    let layout = Layout::read(br#"{"file":"1playerhud","entries":[{"name":"map","values":["player1map"],"children":[{"name":"NorthSmall","values":["NorthSmall"],"children":[{"name":"position","values":[0,-68]},{"name":"Bitmap","values":["reed"]}]}]}]}"#.as_slice(),
        br#"{"file":"hudtransforms","entries":[]}"#.as_slice()).unwrap();
    let mut catalog = Catalog::default();
    catalog.layout = layout;
    let mut s = Screens::new(catalog, "bf2");
    let mut v = view();
    let mut surface = OwnedUiSurface::default();
    surface.textures.insert("reed".into(), 619);
    let mut arrow = |yaw| {
        v.body = Some(Body {
            subject: Subject::default(),
            role: Role::Trooper,
            health: Gauge::default(),
            stamina: Gauge::default(),
            blocking: false,
            map: [Sample::steady(0.5); 2],
            yaw,
            slots: [None, None],
            reticle: "",
        });
        surface.batch.begin();
        s.draw(&mut surface, &v, [1024., 768.], 0.5);
        let i = surface
            .batch
            .draws
            .iter()
            .find(|d| d.texture == 619)
            .unwrap()
            .first
            / 6
            * 12;
        surface.batch.position[i..i + 12].to_vec()
    };
    let expected = arrow(Sample::steady(std::f32::consts::PI));
    for sign in [1., -1.] {
        let actual = arrow(Sample {
            previous: sign * 3.07,
            current: -sign * 3.07,
        });
        assert!(expected
            .iter()
            .zip(actual)
            .all(|(a, b)| (a - b).abs() < 1e-4));
    }
}

#[test]
fn one_previous_press_moves_back_from_the_last_scoreboard_page() {
    let mut v = view();
    let scores: Vec<_> = (0..64)
        .map(|i| ScoreRow {
            player: i,
            team: if i % 2 == 0 { 7 } else { 12 },
            name: "Fern",
            score: 0,
            kills: 0,
            deaths: 0,
            assists: 0,
            ping: None,
        })
        .collect();
    v.scores = &scores;
    v.phase = Phase::Ended(Outcome::Draw);
    let mut s = Screens::new(Catalog::default(), "bf2");
    let mut surface = OwnedUiSurface::default();
    s.draw(&mut surface, &v, [1024., 768.], 0.);
    for _ in 0..100 {
        s.input(&v, Action::Next);
    }
    let end = s.menu.board_offset;
    assert!((1..32).contains(&end));
    s.input(&v, Action::Previous);
    assert_eq!(s.menu.board_offset, end - 1);
    v.scores = &scores[..2];
    surface.batch.begin();
    s.draw(&mut surface, &v, [1024., 768.], 0.);
    assert_eq!(s.menu.board_offset, 0);
}

#[test]
fn rounded_advances_fit_the_last_letter_before_ellipsizing() {
    use super::paint::{Painter, WHITE};
    let cat = Catalog::default();
    let mut s = OwnedUiSurface::default();
    s.fonts[1].height = 16.;
    for g in s.fonts[1].glyphs.values_mut() {
        g.advance = 10.;
    }
    s.fonts[1].glyphs.get_mut(&90).unwrap().page = 778;
    let mut p = Painter::new(&mut s, &cat, Style::for_theme("bf1"), [1024., 768.]);
    p.text(
        [20., 30.],
        131.,
        16.,
        0,
        1,
        WHITE,
        format_args!("AAAAAAAAAAAAAAAAAAZ"),
    );
    assert!(s.batch.draws.iter().any(|d| d.texture == 778));
}

#[test]
fn score_headings_use_the_space_above_the_player_names_in_small_windows() {
    let mut screens = Screens::new(Catalog::default(), "bf2");
    let mut view = view();
    view.phase = Phase::Ended(Outcome::Draw);
    let mut surface = OwnedUiSurface::new(640, 360);
    let font = &mut surface.fonts[0];
    font.height = 16.;
    for glyph in font.glyphs.values_mut() {
        glyph.advance = 12.;
    }
    // The points heading contains this letter beyond the narrower score field.
    font.glyphs.get_mut(&u16::from(b'n')).unwrap().page = 792;
    screens.draw(&mut surface, &view, [640., 360.], 0.);
    let marked: usize = surface
        .batch
        .draws
        .iter()
        .filter(|d| d.texture == 792)
        .map(|d| d.count / 6)
        .sum();
    assert_eq!(marked, 6);
}

#[test]
fn unsupported_name_characters_draw_one_visible_mark_per_scalar() {
    use super::paint::{Painter, WHITE};
    let cat = Catalog::default();
    let mut s = OwnedUiSurface::default();
    s.fonts[1].glyphs.get_mut(&63).unwrap().page = 777;
    let mut p = Painter::new(&mut s, &cat, Style::for_theme("bf1"), [1024., 768.]);
    p.text([20., 30.], 200., 16., 0, 1, WHITE, format_args!("树𐐀Я"));
    assert_eq!(
        s.batch
            .draws
            .iter()
            .filter(|d| d.texture == 777)
            .map(|d| d.count / 6)
            .sum::<usize>(),
        6
    );
}

#[test]
fn death_has_no_crosshair_and_a_draw_uses_the_neutral_board_title() {
    let mut v = view();
    let mut s = Screens::new(Catalog::default(), "bf2");
    let mut surface = OwnedUiSurface::default();
    s.draw(&mut surface, &v, [1024., 768.], 0.);
    assert_eq!(surface.batch.count, 0);
    v.phase = Phase::Ended(Outcome::Draw);
    surface.fonts[3]
        .glyphs
        .get_mut(&u16::from(b'='))
        .unwrap()
        .page = 734;
    s.draw(&mut surface, &v, [1024., 768.], 0.);
    assert!(!surface.batch.draws.iter().any(|d| d.texture == 734));
}

#[test]
fn the_radar_samples_its_authored_window_about_the_player() {
    let layout=Layout::read(br#"{"file":"1playerhud","entries":[{"name":"map","values":["player1map"],"children":[{"name":"position","values":[110,90]},{"name":"BackdropSmall","values":["BackdropSmall"],"children":[{"name":"texcoords","values":[0.1,0.1,0.5,0.3]}]},{"name":"PlayerDirectionSmall","values":["PlayerDirectionSmall"],"children":[{"name":"Mesh","values":["wedge"]},{"name":"color","values":[194,175,3]},{"name":"alpha","values":[0.3]}]}]}]}"#.as_slice(),br#"{"file":"hudtransforms","entries":[]}"#.as_slice()).unwrap();
    let mut cat = Catalog::default();
    cat.layout = layout;
    let mut s = Screens::new(cat, "bf2");
    let mut surface = OwnedUiSurface::default();
    surface.textures.extend([
        ("paper".into(), 704),
        ("wedge".into(), 705),
        ("far".into(), 706),
    ]);
    let mut v = view();
    v.body = Some(Body {
        subject: Subject::default(),
        role: Role::Trooper,
        health: Gauge::default(),
        stamina: Gauge::default(),
        blocking: false,
        map: [Sample::steady(0.7), Sample::steady(0.4)],
        yaw: Sample::default(),
        slots: [None, None],
        reticle: "",
    });
    let c = Contact {
        subject: Subject::default(),
        team: 7,
        map: [Sample::steady(0.5); 2],
        icon: "far",
    };
    v.contacts = std::slice::from_ref(&c);
    s.draw(&mut surface, &v, [1024., 768.], 0.);
    let index = surface
        .batch
        .draws
        .iter()
        .find(|d| d.texture == 704)
        .unwrap()
        .first
        / 6;
    let uv = &surface.batch.uv[index * 8..index * 8 + 8];
    for (a, b) in uv.iter().zip([0.5, 0.3, 0.5, 0.5, 0.9, 0.3, 0.9, 0.5]) {
        assert!((a - b).abs() < 0.00001);
    }
    let wedge = surface
        .batch
        .draws
        .iter()
        .find(|d| d.texture == 705)
        .unwrap()
        .first
        / 6;
    assert!((surface.batch.tint[wedge * 16 + 3] - 0.3).abs() < 0.00001);
    assert!(!surface.batch.draws.iter().any(|d| d.texture == 706));
}

#[test]
fn equipment_change_uses_the_authored_fades_and_unlimited_ammo_art() {
    let layout=Layout::read(br#"{"file":"1playerhud","entries":[{"name":"text","values":["count"],"children":[{"name":"EventText","values":["player1.weapon1.totalAmmoBullets"]}]},{"name":"text","values":["label"],"children":[{"name":"EventText","values":["player1.weapon1.name"]},{"name":"EventEnable","values":["player1.weapon1.change"]},{"name":"FadeInTime","values":[0.5]},{"name":"FadeHoldTime","values":[1]},{"name":"FadeOutTime","values":[0.5]}]},{"name":"Model3D","values":["player1weapon1ammoInfinite"],"children":[{"name":"Mesh","values":["loop"]},{"name":"EventEnable","values":["player1.weapon1.ammoInfinite"]}]}]}"#.as_slice(),br#"{"file":"hudtransforms","entries":[]}"#.as_slice()).unwrap();
    let mut cat = Catalog::default();
    cat.layout = layout;
    cat.bounds.insert("loop".into(), [-0.3, -0.3, 0.3, 0.3]);
    let mut s = Screens::new(cat, "bf1");
    let mut surface = OwnedUiSurface::default();
    surface.textures.insert("loop".into(), 708);
    surface.fonts[1]
        .glyphs
        .get_mut(&u16::from(b'~'))
        .unwrap()
        .page = 709;
    let mut v = view();
    let slot = Slot {
        changed_at: Some(4),
        kind: SlotKind::Saber,
        name: Caption::Literal("~"),
        icon: "",
        rounds: None,
        reserve: None,
        charge: None,
        ready_at: None,
        clip: None,
        heat: None,
        overheated: false,
        refire: None,
    };
    v.body = Some(Body {
        subject: Subject::default(),
        role: Role::RosterHero,
        health: Gauge::default(),
        stamina: Gauge::default(),
        blocking: false,
        map: [Sample::default(); 2],
        yaw: Sample::default(),
        slots: [Some(slot), None],
        reticle: "",
    });
    for (tick, alpha) in [(4, 0.), (19, 0.5), (65, 1.), (109, 0.5), (125, 0.)] {
        v.tick = tick;
        surface.batch.begin();
        s.draw(&mut surface, &v, [1024., 768.], 0.);
        assert!(surface.batch.draws.iter().any(|d| d.texture == 708));
        let opacity = surface
            .batch
            .draws
            .iter()
            .filter(|d| d.texture == 709)
            .flat_map(|d| {
                (d.first / 6..(d.first + d.count) / 6).map(|i| surface.batch.tint[i * 16 + 3])
            })
            .fold(0., f32::max);
        assert!((opacity - alpha).abs() < 0.001, "tick {tick}: {opacity}");
    }
}

#[test]
fn a_full_battle_observation_keeps_the_reticle_and_target_in_the_batch() {
    let layout=Layout::read(br#"{"file":"1playerhud","entries":[{"name":"MultilineText","values":["feed"],"children":[{"name":"Rect","values":[490,100]},{"name":"EventText","values":["player1.message"]}]},{"name":"group","values":["player1spaceassaultgroup"],"children":[{"name":"Rect","values":[180,120]}]},{"name":"BarBitmap","values":["health"],"children":[{"name":"EventValue","values":["player1.lockOnHealthFraction"]},{"name":"Bitmap","values":["ink"]},{"name":"Rect","values":[76,5]}]},{"name":"Model3D","values":["player1reticule_shape"],"children":[{"name":"Mesh","values":["sight"]}]}]}"#.as_slice(),br#"{"file":"hudtransforms","entries":[]}"#.as_slice()).unwrap();
    let mut cat = Catalog::default();
    cat.layout = layout;
    cat.bounds.insert("sight".into(), [-0.4, -0.4, 0.4, 0.4]);
    let mut s = Screens::new(cat, "bf2");
    let mut surface = OwnedUiSurface::default();
    surface
        .textures
        .extend([("needle".into(), 710), ("ink".into(), 711)]);
    for font in &mut surface.fonts {
        for g in font.glyphs.values_mut() {
            g.advance = 2.;
        }
    }
    let mut v = view();
    v.mode = Mode::SpaceAssault;
    let name = "abcdefghijklmnopqrstuvwxyzabcdef";
    let long = "i".repeat(180);
    let caption = Caption::Literal(&long);
    let health = Gauge {
        value: Sample::steady(1.),
        maximum: 1.,
    };
    v.body = Some(Body {
        subject: Subject::default(),
        role: Role::Trooper,
        health,
        stamina: health,
        blocking: false,
        map: [Sample::steady(0.5); 2],
        yaw: Sample::default(),
        slots: [None, None],
        reticle: "needle",
    });
    v.target = Some(Target {
        subject: Subject::default(),
        team: 12,
        name: caption,
        health: Some(health),
        lock: Lock::Locked,
        progress: health,
        bracket: Some([0.35, 0.4, 0.6, 0.7].map(Sample::steady)),
    });
    let scores: Vec<_> = (0..64)
        .map(|i| ScoreRow {
            player: i,
            team: if i % 2 == 0 { 7 } else { 12 },
            name,
            score: i32::MAX,
            kills: u32::MAX,
            deaths: u32::MAX,
            assists: 0,
            ping: None,
        })
        .collect();
    let objectives = [Objective {
        id: 13,
        audience: Audience::All,
        text: caption,
        complete: false,
    }; 4];
    let timers = [Timer {
        audience: Audience::All,
        name: caption,
        due: u32::MAX,
    }; 3];
    let events = [Event {
        sequence: 1,
        until_tick: u32::MAX,
        audience: Audience::All,
        message: Message::Kill {
            killer: name,
            victim: name,
            weapon: "Elm",
            weapon_name: Some("Elm"),
        },
    }; 4];
    let systems: Vec<_> = (0..12)
        .map(|i| System {
            subject: Subject {
                id: i,
                generation: 1,
            },
            team: if i < 6 { 7 } else { 12 },
            name: caption,
            icon: "",
            hull: health,
            shield: Some(health),
            destroyed: false,
            hud: [0.5; 2],
            marker: true,
            ship: "",
            shield_art: "",
        })
        .collect();
    let contacts = [Contact {
        subject: Subject::default(),
        team: 7,
        map: [Sample::steady(0.5); 2],
        icon: "",
    }; 128];
    v.scores = &scores;
    v.objectives = &objectives;
    v.timers = &timers;
    v.events = &events;
    v.systems = &systems;
    v.contacts = &contacts;
    for mode in [Mode::SpaceAssault, Mode::HeroAssault] {
        v.mode = mode;
        v.vehicle = (mode == Mode::SpaceAssault).then_some(Vehicle {
            subject: Subject::default(),
            seat: Seat::Pilot,
            map_pose: None,
            hull: health,
            speed: health,
            boost: Some(health),
            slots: [None, None],
            landing: Some(caption),
        });
        surface.batch.begin();
        // Most of the batch is already taken by the world's overlays.
        surface.batch.count = bfe_render::interface::QUADS - 1 - 40;
        s.draw(&mut surface, &v, [3840., 2160.], 0.5);
        assert!(surface.batch.draws.iter().any(|d| d.texture == 710));
        assert!(surface.batch.draws.iter().any(|d| d.texture == 711));
        assert!(surface.batch.count < bfe_render::interface::QUADS);
        assert!(s.omitted_quads > 0);
    }
}

#[test]
fn an_equipment_mesh_uses_its_projected_art_ahead_of_its_diffuse_bitmap() {
    let layout=Layout::read(br#"{"file":"1playerhud","entries":[{"name":"text","values":["count"],"children":[{"name":"EventText","values":["player1.weapon1.totalAmmoBullets"]}]},
        {"name":"group","values":["rack"],"children":[{"name":"position","values":[300,500]},
            {"name":"Model3D","values":["player1weapon1mesh"],"children":[{"name":"EventMesh","values":["player1.weapon1.mesh"]},
                {"name":"MeshInfo","values":["pen"],"children":[{"name":"position","values":[-40,30]},{"name":"Scale","values":[0.5,0.5,0.5]}]}]}]}]}"#.as_slice(),br#"{"file":"hudtransforms","entries":[{"name":"TransformNameMesh","values":["badge"],"children":[{"name":"EventOutput","values":["player1.weapon1.mesh"]},{"name":"NameMesh","values":["Quill","pen"]}]}]}"#.as_slice()).unwrap();
    let mut cat = Catalog::default();
    cat.layout = layout;
    cat.meshes.insert("pen".into(), "mesh.pen".into());
    cat.bounds.insert("mesh.pen".into(), [-1., -0.25, 1.5, 0.5]);
    let mut s = Screens::new(cat, "bf1");
    let mut surface = OwnedUiSurface::default();
    surface
        .textures
        .extend([("pen".into(), 721), ("mesh.pen".into(), 722)]);
    surface.assets.resize(723, Default::default());
    surface.assets[722] = bfe_render::interface::Asset {
        texture: 722,
        width: 19.,
        height: 7.,
        ready: true,
    };
    let slot = Slot {
        changed_at: None,
        kind: SlotKind::Weapon,
        name: Caption::Literal("Nib"),
        icon: "Quill",
        rounds: Some(5),
        reserve: None,
        charge: None,
        ready_at: None,
        clip: None,
        heat: None,
        overheated: false,
        refire: None,
    };
    let mut v = view();
    v.body = Some(Body {
        subject: Subject::default(),
        role: Role::Trooper,
        health: Gauge::default(),
        stamina: Gauge::default(),
        blocking: false,
        map: [Sample::default(); 2],
        yaw: Sample::default(),
        slots: [Some(slot), None],
        reticle: "",
    });
    s.draw(&mut surface, &v, [1024., 768.], 0.);
    let first = surface
        .batch
        .draws
        .iter()
        .find(|d| d.texture == 722)
        .unwrap()
        .first
        / 6
        * 12;
    // The model's extent at its placement: origin (260, 530), half scale.
    let quad = &surface.batch.position[first..first + 12];
    let k = 0.5 * super::paint::MODEL_UNIT;
    let xs = [quad[0], quad[6]].map(|x| f64::from(x) + 512.);
    let ys = [quad[1], quad[4]].map(|y| 384. - f64::from(y));
    let near = |a: f64, b: f64| (a - b).abs() < 0.01;
    assert!(near(xs[0].min(xs[1]), 260. - k), "{xs:?}");
    assert!(near(xs[0].max(xs[1]), 260. + 1.5 * k), "{xs:?}");
    assert!(near(ys[0].min(ys[1]), 530. - 0.5 * k), "{ys:?}");
    assert!(near(ys[0].max(ys[1]), 530. + 0.25 * k), "{ys:?}");
    assert!(!surface.batch.draws.iter().any(|d| d.texture == 721));
}

#[test]
fn radar_needs_an_observed_map_position() {
    let layout=Layout::read(br#"{"file":"1playerhud","entries":[{"name":"map","values":["player1map"],"children":[]}]}"#.as_slice(),br#"{"file":"hudtransforms","entries":[]}"#.as_slice()).unwrap();
    let mut cat = Catalog::default();
    cat.layout = layout;
    let mut s = Screens::new(cat, "bf2");
    let mut surface = OwnedUiSurface::default();
    surface.textures.insert("paper".into(), 744);
    let v = view();
    s.draw(&mut surface, &v, [1024., 768.], 0.);
    assert!(!surface.batch.draws.iter().any(|d| d.texture == 744));
}

#[test]
fn rotated_radar_art_observes_the_same_remaining_batch_limit_as_text() {
    let layout=Layout::read(br#"{"file":"1playerhud","entries":[{"name":"map","values":["player1map"],"children":[]}]}"#.as_slice(),br#"{"file":"hudtransforms","entries":[]}"#.as_slice()).unwrap();
    let mut cat = Catalog::default();
    cat.layout = layout;
    let mut s = Screens::new(cat, "bf2");
    let mut surface = OwnedUiSurface::default();
    let mut v = view();
    v.body = Some(Body {
        subject: Subject::default(),
        role: Role::Trooper,
        health: Gauge::default(),
        stamina: Gauge::default(),
        blocking: false,
        map: [Sample::steady(0.5); 2],
        yaw: Sample::steady(1.),
        slots: [None, None],
        reticle: "",
    });
    surface.batch.count = bfe_render::interface::QUADS - 1;
    s.draw(&mut surface, &v, [1024., 768.], 0.);
    assert_eq!(surface.batch.count, bfe_render::interface::QUADS - 1);
}

#[test]
fn turret_seat_draws_heat_ring_symmetric_cooldown_arcs_and_overheat_notice() {
    let layout = Layout::read(
        br#"{"file":"1playerhud","entries":[
            {"name":"group","values":["player1reticule"],"children":[
                {"name":"position","values":[0.5,0.5,0,"Viewport"]},{"name":"Scale","values":[0.7,0.7,1]},
                {"name":"BarSegmented","values":["heat"],"children":[
                    {"name":"Rect","values":[0.13,0.15,"Center","Center","Viewport"]},
                    {"name":"NumSegments","values":[10]},{"name":"AngleStart","values":[-125]},{"name":"AngleEnd","values":[125]},
                    {"name":"EventValue","values":["player1.weapon1.heat"]},
                    {"name":"EventColor","values":["player1.weapon1.heatcolor"]},
                    {"name":"Segment","values":["Segment"],"children":[
                        {"name":"Bitmap","values":["ember"]},{"name":"BitmapRect","values":[0.02,0.01,"Center","Center","Viewport"]},{"name":"alpha","values":[0.65]}]}]},
                {"name":"BarSegmented","values":["cool_l"],"children":[
                    {"name":"Rect","values":[0.13,0.15,"Center","Center","Viewport"]},
                    {"name":"NumSegments","values":[4]},{"name":"AngleStart","values":[-170]},{"name":"AngleEnd","values":[-120]},
                    {"name":"color","values":[240,20,30]},{"name":"EventValue","values":["player1.weapon1.refire"]},
                    {"name":"Segment","values":["Segment"],"children":[
                        {"name":"Bitmap","values":["spark"]},{"name":"BitmapRect","values":[0.01,0.014,"Center","Center","Viewport"]}]}]},
                {"name":"BarSegmented","values":["cool_r"],"children":[
                    {"name":"position","values":[0.13,0,0,"Viewport"]},{"name":"rotation","values":[0,180,0]},
                    {"name":"Rect","values":[0.13,0.15,"Center","Center","Viewport"]},
                    {"name":"NumSegments","values":[4]},{"name":"AngleStart","values":[-170]},{"name":"AngleEnd","values":[-120]},
                    {"name":"color","values":[240,20,30]},{"name":"EventValue","values":["player1.weapon1.refire"]},
                    {"name":"Segment","values":["Segment"],"children":[
                        {"name":"Bitmap","values":["spark"]},{"name":"BitmapRect","values":[0.01,0.014,"Center","Center","Viewport"]}]}]}]},
            {"name":"text","values":["warn"],"children":[
                {"name":"position","values":[0.5,0.7,0,"Viewport"]},{"name":"TextAlignment","values":["Center","Top"]},
                {"name":"text","values":["HOT!"]},{"name":"TextFont","values":["gamefont_medium"]},{"name":"color","values":[220,90,10]},
                {"name":"ColorPulseRate","values":[0.6]},{"name":"EventEnable","values":["player1.weaponsOverheat"]}]}
        ]}"#.as_slice(),
        br#"{"file":"hudtransforms","entries":[
            {"name":"TransformNumberColor","values":["ramp"],"children":[
                {"name":"EventInput","values":["player1.weapon1.heat"]},
                {"name":"EventOutput","values":["player1.weapon1.heatcolor"]},
                {"name":"NumberColor","values":[0,230,240,60]},
                {"name":"NumberColor","values":[1,230,90,60]}]}
        ]}"#.as_slice(),
    )
    .unwrap();
    let mut cat = Catalog::default();
    cat.layout = layout;
    let mut screens = Screens::new(cat, "bf2");
    let mut surface = OwnedUiSurface::default();
    surface
        .textures
        .extend([("ember".into(), 901), ("spark".into(), 902)]);
    surface.fonts[2]
        .glyphs
        .get_mut(&u16::from(b'!'))
        .unwrap()
        .page = 903;
    let base_slot = Slot {
        kind: SlotKind::Weapon,
        name: Caption::Literal("Bellows"),
        icon: "",
        changed_at: None,
        rounds: None,
        reserve: None,
        charge: None,
        ready_at: None,
        clip: None,
        heat: Some(Gauge {
            value: Sample {
                previous: 0.3,
                current: 0.7,
            },
            maximum: 1.,
        }),
        overheated: false,
        refire: None,
    };
    let mut v = view();
    v.vehicle = Some(Vehicle {
        subject: Subject::default(),
        seat: Seat::Gunner,
        map_pose: None,
        hull: Gauge::default(),
        speed: Gauge::default(),
        boost: None,
        slots: [Some(base_slot), None],
        landing: None,
    });
    screens.draw(&mut surface, &v, [1024., 768.], 0.5);
    let heat_quads = |surface: &OwnedUiSurface| -> usize {
        surface
            .batch
            .draws
            .iter()
            .filter(|d| d.texture == 901)
            .map(|d| d.count / 6)
            .sum()
    };
    // Fill 0.5 of 10 segments = 5 segments, no overheat banner.
    assert_eq!(heat_quads(&surface), 5);
    assert!(!surface.batch.draws.iter().any(|d| d.texture == 902));
    assert!(!surface.batch.draws.iter().any(|d| d.texture == 903));

    // Overheated turret shows full heat ring, both mirrored refire arcs, and pulsing overheat text.
    v.vehicle.as_mut().unwrap().slots[0] = Some(Slot {
        heat: Some(Gauge {
            value: Sample::steady(1.),
            maximum: 1.,
        }),
        overheated: true,
        refire: Some(Gauge {
            value: Sample::steady(1.),
            maximum: 1.,
        }),
        ..base_slot
    });
    let overheat_alpha = |surface: &OwnedUiSurface| -> f32 {
        let draw = surface
            .batch
            .draws
            .iter()
            .rev()
            .find(|d| d.texture == 903)
            .unwrap();
        let quad = (draw.first + draw.count) / 6 - 1;
        surface.batch.tint[quad * 16 + 3]
    };
    v.tick = 0;
    surface.batch.begin();
    screens.draw(&mut surface, &v, [1024., 768.], 0.);
    assert_eq!(heat_quads(&surface), 10);
    let refire_quads: usize = surface
        .batch
        .draws
        .iter()
        .filter(|d| d.texture == 902)
        .map(|d| d.count / 6)
        .sum();
    // Two arcs of 4 segments each.
    assert_eq!(refire_quads, 8);
    let early = overheat_alpha(&surface);
    // The notice pulses at its authored rate.
    v.tick = 20;
    surface.batch.begin();
    screens.draw(&mut surface, &v, [1024., 768.], 0.);
    assert!((overheat_alpha(&surface) - early).abs() > 1e-3);
}

#[test]
fn flight_weapon_clip_and_reload_drive_clip_and_refire_rings() {
    let layout = Layout::read(
        br#"{"file":"1playerhud","entries":[
            {"name":"group","values":["player1reticule"],"children":[
                {"name":"position","values":[0.5,0.5,0,"Viewport"]},{"name":"Scale","values":[0.7,0.7,1]},
                {"name":"BarSegmented","values":["clipbar"],"children":[
                    {"name":"Rect","values":[0.12,0.14,"Center","Center","Viewport"]},
                    {"name":"NumSegments","values":[10]},{"name":"AngleStart","values":[-110]},{"name":"AngleEnd","values":[130]},
                    {"name":"EventValue","values":["player1.weapon1.totalClipFraction"]},
                    {"name":"Segment","values":["Segment"],"children":[
                        {"name":"Bitmap","values":["petal"]},{"name":"BitmapRect","values":[0.006,0.012,"Center","Center","Viewport"]},
                        {"name":"color","values":[150,170,210]},{"name":"alpha","values":[0.6]}]}]},
                {"name":"BarSegmented","values":["refire_l"],"children":[
                    {"name":"Rect","values":[0.12,0.15,"Center","Center","Viewport"]},
                    {"name":"NumSegments","values":[4]},{"name":"AngleStart","values":[-170]},{"name":"AngleEnd","values":[-120]},
                    {"name":"color","values":[240,20,30]},{"name":"EventValue","values":["player1.weapon1.refire"]},
                    {"name":"Segment","values":["Segment"],"children":[
                        {"name":"Bitmap","values":["spark"]},{"name":"BitmapRect","values":[0.01,0.014,"Center","Center","Viewport"]}]}]},
                {"name":"BarSegmented","values":["refire_r"],"children":[
                    {"name":"position","values":[0.12,0,0,"Viewport"]},{"name":"rotation","values":[0,180,0]},
                    {"name":"Rect","values":[0.12,0.15,"Center","Center","Viewport"]},
                    {"name":"NumSegments","values":[4]},{"name":"AngleStart","values":[-170]},{"name":"AngleEnd","values":[-120]},
                    {"name":"color","values":[240,20,30]},{"name":"EventValue","values":["player1.weapon1.refire"]},
                    {"name":"Segment","values":["Segment"],"children":[
                        {"name":"Bitmap","values":["spark"]},{"name":"BitmapRect","values":[0.01,0.014,"Center","Center","Viewport"]}]}]},
                {"name":"BarSegmented","values":["secondary"],"children":[
                    {"name":"Rect","values":[0.12,0.15,"Center","Center","Viewport"]},{"name":"Scale","values":[1.1,1.1,1]},
                    {"name":"NumSegments","values":[5]},{"name":"AngleStart","values":[-100]},{"name":"AngleEnd","values":[200]},
                    {"name":"color","values":[190,60,20]},{"name":"EventValue","values":["player1.weapon2.refire"]},
                    {"name":"Segment","values":["Segment"],"children":[
                        {"name":"Bitmap","values":["ember"]},{"name":"BitmapRect","values":[0.007,0.009,"Center","Center","Viewport"]}]}]}]}
        ]}"#.as_slice(),
        br#"{"file":"hudtransforms","entries":[]}"#.as_slice(),
    )
    .unwrap();
    let mut cat = Catalog::default();
    cat.layout = layout;
    let mut screens = Screens::new(cat, "bf2");
    let mut surface = OwnedUiSurface::default();
    surface.textures.extend([
        ("petal".into(), 911),
        ("spark".into(), 912),
        ("ember".into(), 913),
    ]);
    let count = |surface: &OwnedUiSurface, tex: u32| -> usize {
        surface
            .batch
            .draws
            .iter()
            .filter(|d| d.texture == tex)
            .map(|d| d.count / 6)
            .sum()
    };
    let primary = Slot {
        kind: SlotKind::Weapon,
        name: Caption::Literal("Slingshot"),
        icon: "",
        changed_at: None,
        rounds: Some(8),
        reserve: None,
        charge: None,
        ready_at: None,
        clip: Some(Gauge {
            value: Sample::steady(8.),
            maximum: 8.,
        }),
        heat: None,
        overheated: false,
        refire: None,
    };
    let secondary = Slot {
        name: Caption::Literal("Shells"),
        rounds: None,
        clip: None,
        refire: Some(Gauge {
            value: Sample {
                previous: 0.4,
                current: 0.8,
            },
            maximum: 1.,
        }),
        ..primary
    };
    let mut v = view();
    v.vehicle = Some(Vehicle {
        subject: Subject::default(),
        seat: Seat::Pilot,
        map_pose: None,
        hull: Gauge::default(),
        speed: Gauge::default(),
        boost: None,
        slots: [Some(primary), Some(secondary)],
        landing: None,
    });
    // Full clip: 10 clip segments, no primary refire, 3 of 5 secondary segments at 0.6.
    screens.draw(&mut surface, &v, [1024., 768.], 0.5);
    assert_eq!(count(&surface, 911), 10);
    assert_eq!(count(&surface, 912), 0);
    assert_eq!(count(&surface, 913), 3);

    // Reloading empty clip: clip ring absent, primary refire at 0.5 draws 2 of 4 segments per side.
    v.vehicle.as_mut().unwrap().slots[0] = Some(Slot {
        rounds: Some(0),
        clip: Some(Gauge {
            value: Sample::steady(0.),
            maximum: 8.,
        }),
        refire: Some(Gauge {
            value: Sample {
                previous: 0.3,
                current: 0.7,
            },
            maximum: 1.,
        }),
        ..primary
    });
    surface.batch.begin();
    screens.draw(&mut surface, &v, [1024., 768.], 0.5);
    assert_eq!(count(&surface, 911), 0);
    assert_eq!(count(&surface, 912), 4);
}

#[test]
fn kill_messages_use_the_games_wording_and_credit_unmanned_guns() {
    let layout = Layout::read(
        br#"{"file":"1playerhud","entries":[{"name":"MultilineText","values":["feed"],"children":[{"name":"Rect","values":[480,100]},{"name":"EventText","values":["player1.message"]},
            {"name":"format","values":["Format"],"children":[{"name":"color","values":[90,200,40]}]}]}]}"#
            .as_slice(),
        br#"{"file":"hudtransforms","entries":[]}"#.as_slice(),
    )
    .unwrap();
    let mut cat = Catalog::default();
    cat.layout = layout;
    let mut s = Screens::new(cat, "bf2");
    let mut surface = OwnedUiSurface::default();
    for (ch, page) in [(b'A', 921), (b'g', 922), (b'K', 923), (b'>', 924), (b'S', 925)] {
        surface.fonts[1]
            .glyphs
            .get_mut(&u16::from(ch))
            .unwrap()
            .page = page;
    }
    let first = |surface: &OwnedUiSurface, tex: u32| -> Option<(f32, [f32; 4])> {
        surface.batch.draws.iter().find(|d| d.texture == tex).map(|d| {
            let q = d.first / 6;
            let t = &surface.batch.tint[q * 16..q * 16 + 4];
            (surface.batch.position[q * 12], [t[0], t[1], t[2], t[3]])
        })
    };
    let kill = |killer| Event {
        sequence: 1,
        until_tick: 400,
        audience: Audience::All,
        message: Message::Kill {
            killer,
            victim: "Kestrel",
            weapon: "kit.scarecrow|kit.other",
            weapon_name: Some("Deck Cannon"),
        },
    };
    let mut v = view();
    let events = [kill("Alder")];
    v.events = &events;
    // Without a prepared wording the message still names both, in the format's colour.
    s.draw(&mut surface, &v, [1024., 768.], 0.);
    let (a, tint) = first(&surface, 921).unwrap();
    assert!(a < first(&surface, 923).unwrap().0);
    assert!((tint[1] - 200. / 255.).abs() < 1e-3);
    assert!(first(&surface, 924).is_none());
    // The prepared wording and the gun's name from the keys its producer supplies.
    s.catalog.push_language([
        ("game.infomsg.killed", "%s glimmers at %s"),
        ("kit.scarecrow", "Scarecrow"),
    ]);
    let events = [kill("")];
    v.events = &events;
    surface.batch.begin();
    s.draw(&mut surface, &v, [1024., 768.], 0.);
    let scarecrow = first(&surface, 925).unwrap().0;
    let glimmers = first(&surface, 922).unwrap().0;
    assert!(scarecrow < glimmers && glimmers < first(&surface, 923).unwrap().0);
}

#[test]
fn on_foot_lock_draws_four_corner_bracket_without_missile_progress_bar() {
    let layout = Layout::read(
        br#"{"file":"1playerhud","entries":[
            {"name":"Bitmap","values":["LockedOnScreen"],"children":[
                {"name":"Bitmap","values":["corner_art"]},
                {"name":"BitmapRect","values":[0.032,0.038,"Right","Bottom","Viewport"]},
                {"name":"texcoords","values":[0.1,0.15,0.6,0.65]},
                {"name":"color","values":[215,65,110]}]}
        ]}"#
        .as_slice(),
        br#"{"file":"hudtransforms","entries":[]}"#.as_slice(),
    )
    .unwrap();
    let mut cat = Catalog::default();
    cat.layout = layout;
    let mut screens = Screens::new(cat, "bf2");
    let mut surface = OwnedUiSurface::default();
    surface.textures.insert("corner_art".into(), 931);
    let quads = |surface: &OwnedUiSurface, tex: u32| -> usize {
        surface
            .batch
            .draws
            .iter()
            .filter(|d| d.texture == tex)
            .map(|d| d.count / 6)
            .sum()
    };
    let mut v = view();
    let saber = Slot {
        kind: SlotKind::Saber,
        name: Caption::Literal("Blade"),
        icon: "",
        changed_at: None,
        rounds: None,
        reserve: None,
        charge: None,
        ready_at: None,
        clip: None,
        heat: None,
        overheated: false,
        refire: None,
    };
    v.body = Some(Body {
        subject: Subject::default(),
        role: Role::RosterHero,
        health: Gauge::default(),
        stamina: Gauge::default(),
        blocking: false,
        map: [Sample::steady(0.5); 2],
        yaw: Sample::default(),
        slots: [Some(saber), None],
        reticle: "",
    });
    // Unlocked: no bracket quads and no solid quads.
    let solid = surface.solid;
    screens.draw(&mut surface, &v, [1280., 720.], 0.5);
    assert_eq!(quads(&surface, 931), 0);
    assert_eq!(quads(&surface, solid), 0);

    // On-foot lock (progress.maximum == 0): 4 corner bracket quads and no solid progress strip.
    v.target = Some(Target {
        subject: Subject {
            id: 4,
            generation: 1,
        },
        team: 12,
        name: Caption::Literal("Sorrel"),
        health: None,
        lock: Lock::Locked,
        progress: Gauge::default(),
        bracket: Some([0.42, 0.36, 0.58, 0.64].map(Sample::steady)),
    });
    surface.batch.begin();
    screens.draw(&mut surface, &v, [1280., 720.], 0.5);
    assert_eq!(quads(&surface, 931), 4);
    assert_eq!(quads(&surface, solid), 0);

    // A missile lock draws the same four corners: the definition has no progress strip.
    v.target.as_mut().unwrap().progress = Gauge {
        value: Sample::steady(1.),
        maximum: 1.,
    };
    surface.batch.begin();
    screens.draw(&mut surface, &v, [1280., 720.], 0.5);
    assert_eq!(quads(&surface, 931), 4);
    assert_eq!(quads(&surface, solid), 0);
}

#[test]
fn kill_feed_uses_localized_forms_and_pipe_fallback_without_weapon_labels() {
    use bfe_render::hud_model2::catalog::text_id;
    let root = std::env::temp_dir().join(format!("bfe-kill-feed-keys-{}", std::process::id()));
    let hud = root.join("common/hud");
    std::fs::create_dir_all(&hud).unwrap();
    std::fs::write(
        hud.join("1playerhud.json"),
        br#"{"file":"1playerhud","entries":[{"name":"MultilineText","values":["feed"],"children":[{"name":"position","values":[40,30]},{"name":"Rect","values":[460,90]},{"name":"EventText","values":["player1.message"]}]}]}"#,
    )
    .unwrap();
    std::fs::write(
        hud.join("hudtransforms.json"),
        br#"{"file":"hudtransforms","entries":[]}"#,
    )
    .unwrap();
    let entity_id = format!("{:08x}", text_id("entity.inv.bldg_deck_gun"));
    let killed_id = format!("{:08x}", text_id("game.infomsg.killed"));
    let defeated_id = format!("{:08x}", text_id("game.infomsg.defeated"));
    let died_id = format!("{:08x}", text_id("game.infomsg.died"));
    let suicide_id = format!("{:08x}", text_id("game.infomsg.suicide"));
    std::fs::write(
        hud.join("english.json"),
        format!(
            r#"{{"a1b2c3d4":"Sentry Gun\r\n","{entity_id}":"Hull Battery\r\n","{killed_id}":"%s ZAPPED %s\r\n","{defeated_id}":"%s BESTED %s\r\n","{died_id}":"%s VANISHED\r\n","{suicide_id}":"%s SLIPPED\r\n"}}"#
        ),
    )
    .unwrap();
    std::fs::write(
        hud.join("presentation.json"),
        br#"{"trees":{"1playerhud":"1playerhud.json","hudtransforms":"hudtransforms.json"},"textures":{},"meshes":{},"languages":{"english":"english.json"}}"#,
    )
    .unwrap();
    let cat = Catalog::open(&root, "english").unwrap();
    std::fs::remove_dir_all(&root).unwrap();

    let mut s = Screens::new(cat, "bf2");
    let mut surface = OwnedUiSurface::default();
    for (ch, page) in [
        (b'S', 941),
        (b'H', 942),
        (b'Z', 943),
        (b'K', 944),
        (b'A', 945),
        (b'(', 946),
        (b'?', 947),
        (b'B', 948),
        (b'V', 949),
        (b'P', 950),
    ] {
        surface.fonts[1]
            .glyphs
            .get_mut(&u16::from(ch))
            .unwrap()
            .page = page;
    }
    let first_x = |surface: &OwnedUiSurface, tex: u32| -> Option<f32> {
        surface
            .batch
            .draws
            .iter()
            .find(|d| d.texture == tex)
            .map(|d| surface.batch.position[d.first / 6 * 12])
    };

    // Pipe-joined pair resolves an unmanned turret hex id ahead of the fallback entity.* key.
    let mut v = view();
    v.mode = Mode::SpaceAssault;
    let turret_match = Event {
        sequence: 1,
        until_tick: 400,
        audience: Audience::All,
        message: Message::Kill {
            killer: "",
            victim: "Kestrel",
            weapon: "a1b2c3d4|entity.inv.bldg_deck_gun",
            weapon_name: None,
        },
    };
    v.events = std::slice::from_ref(&turret_match);
    s.draw(&mut surface, &v, [1024., 768.], 0.);
    // "Sentry Gun ZAPPED Kestrel" (trimmed, no weapon suffix).
    assert!(first_x(&surface, 941).unwrap() < first_x(&surface, 943).unwrap());
    assert!(first_x(&surface, 943).unwrap() < first_x(&surface, 944).unwrap());
    assert!(first_x(&surface, 942).is_none());
    assert!(first_x(&surface, 946).is_none());
    assert!(first_x(&surface, 947).is_none());

    // Missing primary id falls back across '|' to the turret entity.* localization.
    let entity_fallback = Event {
        sequence: 2,
        until_tick: 400,
        audience: Audience::All,
        message: Message::Kill {
            killer: "",
            victim: "Kestrel",
            weapon: "deadbeef|entity.inv.bldg_deck_gun",
            weapon_name: None,
        },
    };
    v.events = std::slice::from_ref(&entity_fallback);
    surface.batch.begin();
    s.draw(&mut surface, &v, [1024., 768.], 0.);
    // "Hull Battery ZAPPED Kestrel".
    assert!(first_x(&surface, 942).unwrap() < first_x(&surface, 943).unwrap());
    assert!(first_x(&surface, 943).unwrap() < first_x(&surface, 944).unwrap());
    assert!(first_x(&surface, 941).is_none());
    assert!(first_x(&surface, 946).is_none());
    assert!(first_x(&surface, 947).is_none());

    // Manned Space Assault kill prints names only ("Alder ZAPPED Kestrel") and never appends a weapon.
    let manned_space = Event {
        sequence: 3,
        until_tick: 400,
        audience: Audience::All,
        message: Message::Kill {
            killer: "Alder",
            victim: "Kestrel",
            weapon: "a1b2c3d4|entity.inv.bldg_deck_gun",
            weapon_name: Some("Hull Battery"),
        },
    };
    v.events = std::slice::from_ref(&manned_space);
    surface.batch.begin();
    s.draw(&mut surface, &v, [1024., 768.], 0.);
    assert!(first_x(&surface, 945).unwrap() < first_x(&surface, 943).unwrap());
    assert!(first_x(&surface, 943).unwrap() < first_x(&surface, 944).unwrap());
    assert!(first_x(&surface, 941).is_none());
    assert!(first_x(&surface, 942).is_none());
    assert!(first_x(&surface, 946).is_none());
    assert!(first_x(&surface, 947).is_none());

    // Manned Hero Assault kill uses game.infomsg.defeated ("Alder BESTED Kestrel"), names only.
    v.mode = Mode::HeroAssault;
    surface.batch.begin();
    s.draw(&mut surface, &v, [1024., 768.], 0.);
    assert!(first_x(&surface, 945).unwrap() < first_x(&surface, 948).unwrap());
    assert!(first_x(&surface, 948).unwrap() < first_x(&surface, 944).unwrap());
    assert!(first_x(&surface, 943).is_none());
    assert!(first_x(&surface, 946).is_none());

    // Unattributed death uses game.infomsg.died ("Kestrel VANISHED").
    let died = Event {
        sequence: 4,
        until_tick: 400,
        audience: Audience::All,
        message: Message::Kill {
            killer: "",
            victim: "Kestrel",
            weapon: "deadbeef|entity.inv.missing_gun",
            weapon_name: None,
        },
    };
    v.events = std::slice::from_ref(&died);
    surface.batch.begin();
    s.draw(&mut surface, &v, [1024., 768.], 0.);
    assert!(first_x(&surface, 944).unwrap() < first_x(&surface, 949).unwrap());
    assert!(first_x(&surface, 943).is_none());

    // Self-kill uses game.infomsg.suicide ("Kestrel SLIPPED").
    let suicide = Event {
        sequence: 5,
        until_tick: 400,
        audience: Audience::All,
        message: Message::Kill {
            killer: "Kestrel",
            victim: "Kestrel",
            weapon: "",
            weapon_name: None,
        },
    };
    v.events = std::slice::from_ref(&suicide);
    surface.batch.begin();
    s.draw(&mut surface, &v, [1024., 768.], 0.);
    assert!(first_x(&surface, 944).unwrap() < first_x(&surface, 950).unwrap());
    assert!(first_x(&surface, 943).is_none());
}
