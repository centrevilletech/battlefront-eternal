use super::*;
fn model() -> HudModel {
    let mut h = HudModel::new();
    for (i, a) in h.assets.iter_mut().enumerate() {
        a.texture = i as i32;
        a.ready = true;
        a.width = 32.;
        a.height = 32.;
    }
    h.input[H::HEIGHT] = 768.;
    h.input[H::COUNTDOWN] = f64::NAN;
    h.input[H::FOV] = 48.;
    h.input[H::ASPECT] = 4. / 3.;
    h
}
fn command(h: &mut HudModel, kind: f64, args: [f64; 4]) {
    h.event(kind as u32, args[0], args[1], args[2], args[3]);
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-10, "{a} != {b}");
}
fn unit(h: &mut HudModel, index: usize, x: f64, z: f64, team: f64) {
    let o = UNITS + index * U::WORDS;
    h.input[o + U::ID] = (index + 1) as f64;
    h.input[o + U::GENERATION] = 1.;
    h.input[o + U::X] = x;
    h.input[o + U::Z] = z;
    h.input[o + U::VISUAL_X] = x;
    h.input[o + U::VISUAL_Z] = z;
    h.input[o + U::TEAM] = team;
    h.input[o + U::ALIVE] = 1.;
    h.input[o + U::HAS_POSITION] = 1.;
    h.input[o + U::TEXTURE] = 11.;
    h.input[o + U::SCALE] = 1.4;
    h.input[o + U::SOLDIER] = 1.;
    h.input[TEAMS + team as usize * T::WORDS + T::TICKETS] = 100.;
    h.input[TEAMS + team as usize * T::WORDS + T::LIVING] = f64::NAN;
    h.input[H::UNITS] = (index + 1) as f64;
}
#[test]
fn health_stats_hits_and_message_expiry_preserve_display_clocks() {
    let mut h = model();
    command(&mut h, 2., [0.8, 0., 0., 0.]);
    command(&mut h, 2., [0.795, 0., 0., 0.]);
    assert_eq!(h.shown_health, 0.8);
    assert_eq!(h.health_flash, 0.);
    command(&mut h, 2., [0.78, 0., 0., 0.]);
    assert_eq!(h.health_flash, 1.);
    // A repeated final health still flashes if a preview first restored it.
    h.health_flash = 0.;
    command(&mut h, C::HEALTH as f64, [1., 0., 0., 0.]);
    command(&mut h, C::HEALTH as f64, [0.78, 0., 0., 0.]);
    assert_eq!(h.health_flash, 1.);
    h.input[H::DT] = 0.1;
    h.update();
    close(h.health_flash, 0.8);
    close(h.health_fader.alpha, 0.8);
    command(&mut h, 7., [2., 0., 1., 0.]);
    assert_eq!(h.stats_left, 0.);
    command(&mut h, 7., [2., 1., 2., 3.]);
    assert_eq!(h.stats, [0., 2., 3.]);
    assert_eq!(h.stat_changes, [-1., 1., 1.]);
    assert_eq!(h.stats_left, 3.);
    command(&mut h, 11., [2., 0., 0., 0.]);
    command(&mut h, 11., [3., 0., 0., 0.]);
    command(&mut h, 11., [1., 0., 0., 0.]);
    assert_eq!(h.hit, [1., 0.5, 0.]);
    h.input[H::DT] = 0.55;
    h.update();
    close(h.hit[0], 0.45);
    assert_eq!(h.hit[1], 0.);
    let message = h.add_text("hello", true);
    for _ in 0..7 {
        command(&mut h, 8., [message as f64, 2., 0., 0.]);
    }
    assert_eq!(h.message_count, 5);
    assert_eq!(h.scroll, 20.);
    h.input[H::DT] = 9.99;
    h.update();
    assert_eq!(h.message_count, 5);
    h.input[H::DT] = 0.02;
    h.update();
    assert_eq!(h.message_count, 0);
}
#[test]
fn radar_scan_transmit_ticket_and_generation_rules_are_strict() {
    let mut h = model();
    unit(&mut h, 0, 20., 0., 1.);
    unit(&mut h, 1, 30., 0., 1.);
    unit(&mut h, 2, 40., 0., 2.);
    h.input[H::HAS_VIEWER] = 1.;
    h.input[H::HAS_CONTROL] = 1.;
    h.input[H::VIEWER_TEAM] = 1.;
    h.input[H::VIEWER] = 1.;
    h.input[H::CONTROLLABLE] = 1.;
    h.input[H::VIEWER] = 1.;
    h.input[UNITS + U::SCAN] = 5.;
    h.input[H::CONTROL_SCAN] = 5.;
    h.input[H::CONTROL_X] = 20.;
    h.input[TEAMS + T::WORDS + T::RELATION] = 1.;
    let tx = UNITS + U::WORDS;
    h.input[tx + U::SCAN] = 10.;
    h.input[tx + U::TRANSMIT] = 10.;
    h.input[TEAMS + T::WORDS + T::RELATION] = 1.;
    h.update_radar(0.);
    assert_eq!(h.radar[3].alpha, 0.);
    h.input[UNITS + 2 * U::WORDS + U::X] = 39.9;
    h.update_radar(0.);
    assert_eq!(h.radar[3].alpha, 1.);
    h.input[UNITS + 2 * U::WORDS + U::X] = 40.;
    h.update_radar(1.);
    close(h.radar[3].alpha, 223. / 255.);
    h.input[UNITS + 2 * U::WORDS + U::GENERATION] = 2.;
    h.update_radar(0.);
    assert_eq!(h.radar[3].alpha, 0.);
    h.shot(3, 2., 0.2);
    h.time = 0.01;
    h.update_radar(0.01);
    assert!(h.radar_icons.last().unwrap().color[1] > 0.9);
    h.input[UNITS + 2 * U::WORDS + U::GENERATION] = 3.;
    h.input[TEAMS + 2 * T::WORDS + T::TICKETS] = 1.;
    h.update_radar(0.);
    assert_eq!(h.radar[3].alpha, 1.);
}
#[test]
fn rendered_heading_does_not_change_sensor_discovery_and_poles_retain_yaw() {
    let mut h = model();
    unit(&mut h, 0, 0., 0., 1.);
    unit(&mut h, 1, 100., 0., 2.);
    h.input[H::HAS_VIEWER] = 1.;
    h.input[H::HAS_CONTROL] = 1.;
    h.input[H::VIEWER_TEAM] = 1.;
    h.input[H::VIEWER] = 1.;
    h.input[H::CONTROLLABLE] = 1.;
    h.input[UNITS + U::SCAN] = 10.;
    h.input[H::CONTROL_SCAN] = 10.;
    h.input[UNITS + U::WORDS + U::VISUAL_X] = 0.;
    h.update_radar(0.);
    assert_eq!(h.radar[2].alpha, 0.);
    let o = UNITS + U::WORDS;
    h.input[o + U::YAW] = 0.7;
    close(h.heading(o, 2), 0.7);
    h.input[o + U::FORWARD] = 1.;
    h.input[o + U::FORWARD_X] = 0.;
    h.input[o + U::FORWARD_Z] = 0.;
    close(h.heading(o, 2), 0.7);
    h.input[o + U::FORWARD_X] = 1.;
    close(h.heading(o, 2), PI / 2.);
    h.input[o + U::HAS_QUATERNION] = 1.;
    h.input[o + U::QUATERNION..o + U::QUATERNION + 4].copy_from_slice(&[0., 1., 0., 0.]);
    close(h.heading(o, 2), PI);
}
#[test]
fn flight_prompts_minimap_range_and_localized_kill_feed_are_native() {
    let mut h = model();
    for i in 0..7 {
        h.input[H::PROMPT_TEXTS + i] = (i + 1) as f64;
    }
    h.input[H::FLIGHT_PILOT] = 1.;
    h.input[H::FLIGHT_STATE] = 1.;
    assert_eq!(h.prompt_line(), 1.);
    h.input[H::FLIGHT_WARNING] = 1.;
    assert_eq!(h.prompt_line(), 3.);
    h.input[H::FLIGHT_PILOT] = 0.;
    assert_eq!(h.prompt_line(), 0.);
    // Feed both names as data; a Unicode name remains a UTF-16 glyph sequence.
    let template = h.add_text("%s KILLED %s", true);
    let killer = h.add_text("PILOT", true);
    let victim = h.add_text("DROID", true);
    h.input[H::KILL_TEMPLATES + 3] = template as f64;
    command(&mut h, 15., [3., killer as f64, victim as f64, 2.]);
    assert_eq!(String::from_utf16(&h.messages[0].line[..h.messages[0].len]).unwrap(), "PILOT KILLED DROID");
}
#[test]
fn frame_draw_order_covers_death_layer_overlay_and_weapon_channel_deduplication() {
    let mut h = model();
    h.input[H::BATTLE] = 1.;
    h.input[H::OWN] = 120.;
    h.input[H::OWN_START] = 150.;
    h.input[H::FOE] = 100.;
    h.input[H::FOE_START] = 150.;
    h.input[H::RETICLE] = 1.;
    h.input[H::RETICLE_X] = 0.5;
    h.input[H::RETICLE_Y] = 0.4;
    for ch in 2..4 {
        let o = WEAPONS + ch * W::WORDS;
        h.input[o + W::ID] = 1.;
        h.input[o + W::RETICLE] = 20.;
    }
    h.build(0);
    assert_eq!(h.batch.count, 4, "team panel, two bars, shared reticle once");
    assert_eq!(h.batch.draws[0].texture, HUD2);
    assert_eq!(h.batch.draws[1].texture, 20);
    h.input[H::DEAD] = 1.;
    h.build(0);
    assert_eq!(h.batch.count, 3);
    h.input[H::LAYER] = 1.;
    h.input[H::HIDE_TEAMS] = 1.;
    h.build(0);
    assert_eq!(h.batch.count, 0);
    h.input[H::HIDE_TEAMS] = 0.;
    h.input[H::COVERED] = 1.;
    h.build(0);
    assert_eq!(h.batch.count, 0);
}

#[test]
fn objective_capture_and_map_blinks_consume_raw_post_counts() {
    let mut h = model();
    h.input[H::POSTS] = 1.; h.input[H::CAPTURE_POST] = 0.; h.input[H::PLAYER_TEAM] = 1.;
    h.input[POSTS + P::TEAM] = 2.; h.input[POSTS + P::RELATION] = 2.; h.input[POSTS + P::NEUTRALIZE] = 0.4;
    h.batch.begin(); h.capture();
    assert_eq!(h.batch.count, 2);
    assert_eq!(&h.batch.tint[..4], &GRAY.map(|v| v as f32));
    assert_eq!(&h.batch.tint[16..20], &RED.map(|v| v as f32));
    // A neutral contested post fills from the capturing team's colour. The
    // other positive count supplies the top colour only on its half-second.
    h.input[POSTS + P::TEAM] = 0.; h.input[POSTS + P::PROGRESS] = 0.25;
    h.input[POSTS + P::TOWARD] = 2.; h.input[POSTS + P::TOWARD_RELATION] = 2.; h.input[POSTS + P::CONTESTED] = 1.;
    h.input[POSTS + P::COUNTS] = POST_COUNTS as f64; h.input[POSTS + P::COUNTS_LENGTH] = 1.;
    h.input[POST_COUNTS + PC::TEAM] = 1.; h.input[POST_COUNTS + PC::COUNT] = 1.; h.input[POST_COUNTS + PC::RELATION] = 1.;
    h.time = 0.75; h.batch.begin(); h.capture();
    assert_eq!(&h.batch.tint[..4], &GREEN.map(|v| v as f32));
    assert_eq!(&h.batch.tint[16..20], &RED.map(|v| v as f32));
    h.time = 1.; h.batch.begin(); h.capture(); assert_eq!(&h.batch.tint[..4], &GRAY.map(|v| v as f32));
}

#[test]
fn charge_heat_and_acquisition_keep_panel_priority_and_authored_uv_slices() {
    let mut h = model(); h.weapon_fader.alpha = 1.; h.input[H::ARSENAL] = 1.;
    h.input[WEAPONS + W::ID] = 1.; h.input[WEAPONS + W::BOTTOMLESS] = 1.;
    h.input[WEAPONS + W::CHARGING] = 1.; h.input[WEAPONS + W::HELD] = 1.; h.input[WEAPONS + W::MAX_PRESSED] = 2.;
    h.input[WEAPONS + W::HEAT_PER_SHOT] = 0.25;
    h.batch.begin(); h.weapons();
    assert_eq!(h.batch.count, 2); assert_eq!(h.batch.uv[14], 0.5);
    assert_eq!(&h.batch.tint[16..20], &GREEN.map(|v| v as f32));
    // Releasing a held charge restores heat presentation without a new panel.
    h.input[WEAPONS + W::CHARGING] = 0.; h.input[WEAPONS + W::HEAT] = 0.75; h.input[WEAPONS + W::OVERHEATED] = 1.;
    h.batch.begin(); h.weapons(); assert_eq!(h.batch.uv[14], 0.6640625);
    assert_eq!(&h.batch.tint[16..20], &RED.map(|v| v as f32));
    h.input[WEAPONS + W::LOCK] = 1.; h.input[WEAPONS + W::LOCK_TARGET] = 2.;
    h.input[WEAPONS + W::LOCK_ELAPSED] = 0.5; h.input[WEAPONS + W::LOCK_TIME] = 2.;
    h.batch.begin(); h.weapons(); assert_eq!(h.batch.uv[14], 0.6640625);
    assert_eq!(&h.batch.tint[16..20], &GREEN.map(|v| v as f32));
}

#[test]
fn cursor_has_eight_aim_ticks_four_diamond_edges_and_seat_only_spread_fallback() {
    let mut h = model(); h.input[H::RETICLE] = 1.; h.input[H::RETICLE_X] = 0.5; h.input[H::RETICLE_Y] = 0.6;
    let w = WEAPONS + 2 * W::WORDS; h.input[w + W::ID] = 1.; h.input[w + W::RETICLE] = 20.; h.input[w + W::LOCK_OFF_ANGLE] = 8.;
    h.assets[20].width = 64.; h.assets[20].height = 64.;
    h.batch.begin(); h.reticle(); assert_eq!(h.batch.count, 9);
    assert_eq!(h.batch.position[0], -32.); assert_eq!(h.batch.position[6], 32.);
    h.input[H::MARKER] = 1.; h.input[H::MARKER_X] = 0.7; h.input[H::MARKER_Y] = 0.4; h.input[H::MARKER_PROGRESS] = 0.5;
    h.lock_diamond(); assert_eq!(h.batch.count, 13); assert_eq!(h.batch.tint[9 * 16 + 3], (75. / 255.) as f32);
    h.input[w + W::LOCK_OFF_ANGLE] = 0.; h.batch.begin(); h.reticle(); h.lock_diamond(); assert_eq!(h.batch.count, 1);
    // No-art reticles are four spread ticks for an occupied vehicle only.
    h.input[w + W::RETICLE] = 0.; h.input[H::VEHICLE] = 1.; h.batch.begin(); h.reticle(); assert_eq!(h.batch.count, 4);
    h.input[H::VEHICLE] = 0.; h.batch.begin(); h.reticle(); assert_eq!(h.batch.count, 0);
    h.input[w + W::RETICLE] = 20.; h.input[w + W::LOCK_OFF_ANGLE] = 8.; h.input[H::ASPECT] = 16. / 9.; h.input[H::FOV] = 65.;
    h.batch.begin(); h.reticle(); let wide = h.batch.position[12];
    h.input[H::FOV] = 35.; h.batch.begin(); h.reticle(); assert!(h.batch.position[12] > wide);
}

#[test]
fn stalled_publication_and_event_bursts_preserve_final_state_and_feed_order() {
    let mut h = model();
    let one = h.add_text("ONE", false);
    let two = h.add_text("TWO", false);
    let template = h.add_text("%S KILLED %S", false);
    h.input[H::KILL_TEMPLATES + 3] = template as f64;
    // A long load can run arbitrary setter/event counts without an update or
    // build. Messages still evict exactly the oldest of the five visible lines.
    for i in 0..100_000 {
        let o = STATES + C::SECOND as usize * S::WORDS;
        h.input[o] = HEALTH as f64;
        h.input[o + 1] = (i % 100) as f64 / 100.;
        h.input[H::STATES] = (1 << C::SECOND) as f64;
        h.event(C::MESSAGE, one as f64, 1., 0., 0.);
        h.event(C::KILL, 3., one as f64, two as f64, 2.);
        h.event(C::HIT, 2., 0., 0., 0.);
        h.event(C::DAMAGE, 1., 0., 0., 0.);
        h.event(C::SHOT, 1., 1., 0.1, 0.);
        h.event(C::TOGGLE_MAP, 0., 0., 0., 0.);
    }
    assert_eq!(h.time, 0.);
    assert_eq!(h.message_count, 5);
    assert_eq!(h.messages[0].len, 14);
    assert_eq!(h.messages[1].text, one as usize);
    assert_eq!(h.messages[4].color, relation(2.));
    assert_eq!(h.hit[0], 50_000.);
    assert_eq!(h.damage[0], 1.);
    assert!(!h.expanded);
    assert!(h.second_woke.is_nan());
    h.commands();
    assert_eq!(h.second_woke, 0.99);
    assert_eq!(h.input[H::STATES], 0.);
    // Repeated updates without a build must continue clocks normally.
    h.input[H::DT] = 1. / 60.;
    for _ in 0..600 { h.update(); }
    h.build(0);
    assert!((h.time - 10.).abs() < 1e-10);
}

#[test]
fn oversized_source_counts_clip_and_reused_radar_leases_forget_prior_owners() {
    let mut h = model();
    for i in 0..MAX_UNITS { unit(&mut h, i, 1., 0., 1.); }
    h.input[TEAMS + T::WORDS + T::RELATION] = 1.;
    h.input[H::UNITS] = (MAX_UNITS * 20) as f64;
    h.input[H::POSTS] = (MAX_POSTS * 20) as f64;
    h.input[H::CAPTURE_POST] = (MAX_POSTS * 10) as f64;
    h.update_radar(0.);
    assert_eq!(h.radar_icons.len(), 450);
    h.build(0);
    assert!(h.batch.count <= QUADS);
    h.shot(1, 1., 1.);
    assert_eq!(h.radar[1].alpha, 1.);
    h.shot(1, 2., 0.1);
    assert_eq!(h.radar[1].generation, 2.);
    assert_eq!(h.radar[1].shots.iter().filter(|s| s.0 > 0.).count(), 1);
    assert_eq!(h.radar[1].shots[0].1, 0.1);
    h.shot(usize::MAX, 1., 1.);
}
