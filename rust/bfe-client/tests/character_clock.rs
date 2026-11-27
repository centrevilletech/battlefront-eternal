//! Character roots and joints must follow the same historical instant as the camera.
use bfe_client::online::clock::Clock;
use bfe_content::{actor::build_actor, json, zero_hash};
use bfe_content_native::animation2::{
    player::{Arena, Player},
    state::{State, Tables},
    Archive, Character, Class,
};
use serde_json::json;

fn character() -> Class {
    let mesh=json::parse(r#"{"bones":[{"name":"lantern","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}],"parts":[]}"#).unwrap();
    let actor = build_actor(&mesh, &[]).unwrap();
    let frames = 241;
    let mut samples = Vec::new();
    let mut clips = Vec::new();
    for (stance, offset) in [("stand", 0.), ("crouch", 0.125)] {
        let name = format!("ochre_test_{stance}_idle_full");
        clips.push(json!({"crc":zero_hash::crc_hex(&name),"flags":0,"offset":samples.len(),"frames":frames,"tracks":[zero_hash::crc_hex("lantern")]}));
        for i in 0..frames {
            samples.extend([0f32, 0., 0., 1., 0., i as f32 / 30. + offset, 0.]);
        }
    }
    let meta =
        json::parse(&json!({"fps":30,"banks":{"ochre":{"clips":clips}}}).to_string()).unwrap();
    let bytes: Vec<_> = samples.into_iter().flat_map(f32::to_le_bytes).collect();
    let archive = Archive::new(&meta, &bytes).unwrap();
    let def = json::parse(r#"{"weapons":[{"AnimationBank":"test"}]}"#).unwrap();
    let animation = Character::bind(
        "traveler",
        &def,
        &["ochre".into()],
        &actor,
        &archive,
        &archive,
    )
    .unwrap();
    let tables = Tables::new(
        &animation,
        &def,
        &bfe_content::Value::Null,
        &bfe_content::Value::Null,
    )
    .unwrap();
    Class {
        side: String::new(),
        name: "traveler".into(),
        model: String::new(),
        animation,
        tables,
    }
}

#[test]
fn both_playout_clocks_keep_root_and_joints_moving_through_a_clip_change() {
    let class = character();
    for interval in [1, 3] {
        for fps in [60, 144, 500] {
            let mut clock = if interval == 1 {
                Clock::default()
            } else {
                Clock::online()
            };
            let dt = 1. / f64::from(fps);
            let mut arena = Arena::new(1, 1).unwrap();
            let mut player = Player::default();
            let mut pose = arena.slot(0, 1);
            let mut next = interval;
            let mut last = None;
            let mut measured = 0;
            let mut first_clip = None;
            let mut changes = 0;
            let mut max_error = 0f64;
            for frame in 1..=fps * 6 {
                let now = f64::from(frame) / f64::from(fps);
                while f64::from(next) / 60. <= now {
                    let s = State {
                        tick: next,
                        position: [f64::from(next) * 0.1, 0., 0.],
                        crouch: next >= 180,
                        ..Default::default()
                    };
                    let clip = class
                        .tables
                        .sample(&class.animation, &s, 0.)
                        .motion
                        .base
                        .unwrap();
                    changes += usize::from(first_clip.replace(clip).is_some_and(|p| p != clip));
                    player.publish(&class, s, &mut pose);
                    clock.arrive(next.into(), now, interval.into()).unwrap();
                    next += interval;
                }
                let shown = clock.advance(dt).unwrap();
                let root = player.place_at(&class, shown, false, &mut pose);
                let x = root.elements[12];
                let y = pose.world[0].elements[13];
                if frame > fps * 2 {
                    let (old_shown, old_x, old_y) = last.unwrap();
                    let elapsed = (shown - old_shown) / 60.;
                    assert!(x>old_x && y>old_y,"frozen at {fps} Hz, packet interval {interval}, shown {shown}, joint {old_y} -> {y}");
                    assert!(
                        y - old_y < elapsed * 4.,
                        "joint jumped at {fps} Hz, tick {shown}"
                    );
                    max_error = max_error.max((x - shown * 0.1).abs());
                    measured += 1;
                }
                last = Some((shown, x, y));
            }
            assert_eq!(changes, 1);
            assert!(max_error < 1e-11);
            println!("{fps} Hz, publication every {interval} ticks, {measured} moving frames, no repeats or jumps, max root error {max_error:e} m");
        }
    }
}

#[test]
fn a_mounted_character_samples_the_seat_clip_in_the_vehicle_frame() {
    let class = character();
    let id = class
        .animation
        .named_clip("ochre_test_crouch_idle_full")
        .unwrap();
    let mut player = Player::default();
    let mut arena = Arena::new(1, 1).unwrap();
    let mut pose = arena.slot(0, 1);
    let state = State {
        tick: 12,
        position: [10., -10., 21.],
        seat: Some((2, 1, 0)),
        seat_clip: Some(id),
        seat_time: 1.25,
        ..State::default()
    };
    player.publish(&class, state, &mut pose);
    let mut seat = three::Matrix4::default();
    seat.make_rotation_z(std::f64::consts::FRAC_PI_2);
    seat.set_position_v3(&three::Vector3::new(3., 4., 5.));
    player.place_on(&class, 12., seat, &mut pose);
    let joint = &pose.world[0].elements;
    assert!((joint[12] - (3. - 1.375)).abs() < 1e-6);
    assert!((joint[13] - 4.).abs() < 1e-6 && (joint[14] - 5.).abs() < 1e-6);
    seat.elements[12] += 1.;
    player.place_on(&class, 12., seat, &mut pose);
    assert!((pose.world[0].elements[12] - (4. - 1.375)).abs() < 1e-6);
}

#[test]
fn discontinuities_seats_and_lives_snap_both_endpoints() {
    use bfe_content_native::animation2::state::Action;
    let class = character();
    let mut arena = Arena::new(1, 1).unwrap();
    let mut pose = arena.slot(0, 1);
    let mut player = Player::default();
    let mut s = State::default();
    player.publish(&class, s, &mut pose);
    let mut observations = 0;
    for (discontinuity, seat, action) in [
        (true, None, Action::None),
        (false, Some((8, 1, 0)), Action::None),
        (false, Some((8, 1, 1)), Action::None),
        (false, Some((8, 2, 1)), Action::None),
        (false, None, Action::None),
        (true, None, Action::Death),
        (false, None, Action::None),
    ] {
        s.tick += 3;
        s.position[0] += 40.;
        s.discontinuity = discontinuity;
        s.seat = seat;
        s.action = action;
        assert!(player.publish(&class, s, &mut pose));
        for i in 0..=25 {
            let t = f64::from(s.tick) - 3. + f64::from(i) * 3. / 25.;
            assert_eq!(
                player.place_at(&class, t, false, &mut pose).elements[12],
                s.position[0]
            );
            assert_eq!(player.state_at(t).seat, s.seat);
            observations += 1;
        }
    }
    s.tick += 1;
    s.discontinuity = true;
    s.yaw += 0.2;
    assert!(!player.publish(&class, s, &mut pose));
    assert!(player.state_at(f64::from(s.tick) - 0.5).yaw < s.yaw);
    player.reset();
    let new_life = State {
        position: [-18., 0., 0.],
        ..Default::default()
    };
    player.publish(&class, new_life, &mut pose);
    assert_eq!(player.state_at(0.5).position, new_life.position);
    println!("{observations} discontinuity samples, zero intermediate positions or seats");
}

#[test]
fn missing_death_holds_the_last_pose_and_falls_once_per_life() {
    use bfe_content_native::animation2::state::Action;
    let class = character();
    let mut arena = Arena::new(1, 1).unwrap();
    let mut pose = arena.slot(0, 1);
    let mut player = Player::default();
    let mut last_height = 0.;
    for tick in 0..180 {
        let dead = (32..135).contains(&tick) || tick >= 150;
        let state = State {
            tick,
            action: if dead { Action::Death } else { Action::None },
            elapsed: f64::from(tick.saturating_sub(if tick >= 150 { 150 } else { 32 })) / 60.,
            ..Default::default()
        };
        player.publish(&class, state, &mut pose);
        player.place_at(&class, tick.into(), false, &mut pose);
        let m = pose.world[0].elements;
        match tick {
            31 => {
                last_height = m[13];
                assert!(last_height > 0.4);
            }
            32 => assert_eq!(m[13], last_height),
            50 => assert!((m[13] - last_height * std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-9),
            68..135 => {
                assert!(m[13].abs() < 1e-9);
                assert!((m[14] + last_height).abs() < 1e-9);
            }
            _ => {}
        }
    }
    assert_eq!(player.fallback_deaths, 2);
}
