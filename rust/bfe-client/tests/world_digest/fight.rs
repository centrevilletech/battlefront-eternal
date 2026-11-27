use bfe_wide::Wide;
use super::{
    allocations, mix, ClientCommand, ClientFlightCommand, ClientLook, PublicationSlot, Runtime,
    CLOCK_START_MS, CLOCK_UNIX_MS,
};

/// Replay ordinary input against a prepared saber class and its opposing infantry.
/// Environment supplies both class IDs, a spawn path and the family root.
#[test]
#[ignore]
fn infantry_fight_replay() {
    use bfe_client::generated_input::ClientHeld;
    use bfe_client::local::{
        context::Native,
        setup::{NativeSource, Source},
    };
    use bfe_content::files::DirFiles;
    use bfe_sim::{
        abi::context::Context,
        combat::combo::ComboButton,
        units::infantry::{
            contact::{Cause, Outcome},
            Controls,
        },
        world::admit::{Bots, Options as WorldOptions},
    };
    let Ok(root) = std::env::var("BFE_FIGHT_CONTENT") else {
        return;
    };
    let unit = std::env::var("BFE_FIGHT_UNIT").unwrap();
    let target = std::env::var("BFE_FIGHT_TARGET").unwrap();
    let files = DirFiles::from_resolved_root(std::path::Path::new(&root), bfe_family::GameFamily::Swbf2);
    let input_file = std::env::var_os("BFE_FIGHT_INPUT");
    let mut script: Vec<(u32, f64, Controls)> = Vec::new();
    if let Some(path) = &input_file {
        for line in std::fs::read_to_string(path).unwrap().lines() {
            let mut fields = line.split_ascii_whitespace();
            let held = fields.next().unwrap().parse().unwrap();
            let x = u64::from_str_radix(fields.next().unwrap(), 16).unwrap();
            let block = fields.next().unwrap().parse().unwrap();
            let melee_held = fields.next().unwrap().parse().unwrap();
            let melee_pressed = fields.next().unwrap().parse().unwrap();
            assert!(fields.next().is_none());
            script.push((
                held,
                f64::from_bits(x),
                Controls {
                    block,
                    melee_held,
                    melee_pressed,
                    ..Default::default()
                },
            ));
        }
    }
    let mut expected = Vec::new();
    let ticks = 3900;
    if input_file.is_some() {
        assert_eq!(script.len(), ticks);
    }
    for run in 0..2 {
        let ctx = Context::new();
        let options = WorldOptions {
            family: bfe_family::GameFamily::Swbf2,
            map: "tat2".into(),
            side: "all".into(),
            mode: "eli".into(),
            unit: Some(unit.clone()),
            practice_target: Some(target.clone()),
            seed: 7,
            bots: Bots::read("classic", "medium", "full"),
            ..Default::default()
        };
        let admitted = NativeSource {
            files: &files,
            options,
            spawn_screen: false,
        }
        .admit(&ctx)
        .unwrap();
        let native = Native::new(ctx.clone(), &admitted).unwrap();
        let mut slot = PublicationSlot::new_preallocated();
        let mut rt =
            Runtime::new_into_slot(native, admitted, CLOCK_START_MS, CLOCK_UNIX_MS, &mut slot)
                .unwrap();
        let mut trace = (run == 0).then(|| std::env::var_os("BFE_BENCH_FIGHT_TRACE")).flatten()
            .map(|p| std::io::BufWriter::new(std::fs::File::create(p).unwrap()));
        let mut cursor = 0;
        let mut hash = 0xcbf2_9ce4_8422_2325;
        let mut input_hash = 0xcbf2_9ce4_8422_2325;
        let mut peaks = 0;
        let mut total = 0;
        let mut kills = 0;
        let mut saber_targets = Vec::new();
        let mut deflects = 0;
        let mut losses = 0;
        let mut owner_id = None;
        let mut killed_life = None;
        let mut replacements = 0;
        let mut next_press = 0;
        let mut release = 0;
        for n in 1..=ticks {
            if run == 0 && input_file.is_none() {
                let (mut actor, mut bots) = (None, [None; 3]);
                let mut count = 0;
                ctx.owned(|w| {
                    w.visit_infantry(|id, h, hp, p, y| {
                        if h {
                            owner_id = Some(id.slot);
                            if hp > 0. {
                                actor = Some((p, y));
                            }
                        } else if hp > 0. && count < 3 {
                            bots[count] = Some(p);
                            count += 1;
                        }
                    })
                })
                .unwrap();
                let mut held = 0_u32;
                let mut mouse = 0.;
                let mut controls = Controls::default();
                if let Some((pos, yaw)) = actor {
                    let distance = |p: &[f64; 3]| (p[0] - pos[0]).hypot(p[2] - pos[2]);
                    if let Some(enemy) = bots
                        .iter()
                        .flatten()
                        .min_by(|a, b| distance(a).total_cmp(&distance(b)))
                    {
                        let d = distance(enemy);
                        let offset = (enemy[0] - pos[0]).atan2(enemy[2] - pos[2]) - yaw;
                        let turn = offset.sin().atan2(offset.cos());
                        if kills == 0 {
                            mouse = (-turn / 0.00293).clamp(-80., 80.);
                            controls.block = deflects == 0;
                            if d > 0.85 && turn.abs() < 0.65 {
                                held = 1 << ClientHeld::Forward as u8;
                            }
                            if !controls.block && d < 3. && turn.abs() < 0.8 && n >= next_press {
                                controls.melee_pressed = ComboButton::Fire.bit();
                                next_press = n + 24;
                                release = n + 5;
                            }
                            if n < release {
                                controls.melee_held = ComboButton::Fire.bit();
                            }
                        }
                    }
                }
                script.push((held, mouse, controls));
            }
            let (held, x, extra) = script[n - 1];
            input_hash = mix(input_hash, &held.to_le_bytes());
            input_hash = mix(input_hash, &x.to_bits().to_le_bytes());
            input_hash = mix(
                input_hash,
                &[u8::from(extra.block), extra.melee_pressed, extra.melee_held],
            );
            let cmd = ClientCommand {
                tick: n as f64,
                sequence: n as f64,
                generation: slot.infantry.map_or(0., |sample| sample.generation),
                held,
                mouse_scale: 1.,
                has_mouse_scale: true,
                aspect: 16.0 / 9.0,
                has_aspect: true,
                look_count: 1,
                ..Default::default()
            };
            rt.submit(
                &cmd,
                &ClientLook { x, y: 0. },
                &ClientFlightCommand {
                    scale: 1.,
                    curve: 1.,
                    ..Default::default()
                },
                rt.tick(),
                None,
                false,
            )
            .unwrap();
            assert_eq!(rt.refusals(), 0, "input refused at tick {n}");
            rt.infantry_input(n as f64, extra).unwrap();
            let a = allocations();
            rt.step_into_slot(&mut slot).unwrap();
            let allocated = allocations() - a;
            if n > 900 {
                peaks = peaks.max(allocated);
                total += allocated;
            }
            ctx.owned(|w| {
                w.visit_infantry(|id, human, hp, pos, yaw| {
                    for n in [id.slot, id.slab, id.generation, u32::from(human)] {
                        hash = mix(hash, &n.to_le_bytes());
                    }
                    for n in [hp, pos[0], pos[1], pos[2], yaw] {
                        hash = mix(hash, &n.to_bits().to_le_bytes());
                    }
                })
            })
            .unwrap();
            hash = mix(hash, &slot.bytes);
            for word in &slot.block.words {
                hash = mix(hash, &word.to_bits().to_le_bytes());
            }
            if let Some(s) = slot.infantry {
                hash = mix(hash, &s.energy.to_bits().to_le_bytes());
                hash = mix(hash, &s.combo.unwrap_or(u16::MAX).to_le_bytes());
                hash = mix(hash, &s.anim_time.to_bits().to_le_bytes());
                assert!(s
                    .saber
                    .is_none_or(|blade| blade.first_omitted_attack.is_none()));
                owner_id = Some(s.id);
            }
            if let Some(file) = &mut trace {
                use std::io::Write;
                ctx.owned(|world| {
                    world.visit_infantry(|id, human, hp, p, yaw| {
                        writeln!(file, "body\t{n}\t{id:?}\t{human}\t{hp}\t{p:?}\t{yaw}").unwrap();
                    });
                }).unwrap();
                if let Some(log) = slot.contacts.as_deref() {
                    for event in log.since(cursor) { writeln!(file, "contact\t{event:?}").unwrap(); }
                }
            }
            if let Some(log) = slot.contacts.as_deref() {
                for e in log.since(cursor) {
                    cursor = e.sequence;
                    for value in [
                        Wide::from_u32(e.sequence),
                        Wide::from_u32(e.tick),
                        e.source.map_or(Wide::MAX, |id| Wide::from_u32(id.slot.into())),
                        e.source.map_or(Wide::MAX, |id| Wide::from_u32(id.slab.into())),
                        e.source.map_or(Wide::MAX, |id| Wide::from_u32(id.generation.into())),
                        Wide::from_u32(e.target.slot.into()),
                        Wide::from_u32(e.target.slab.into()),
                        Wide::from_u32(e.target.generation.into()),
                        Wide::from_f64_bits(e.before),
                        Wide::from_f64_bits(e.after),
                        Wide::from_f64_bits(e.energy_spent),
                        Wide::from_f64_bits(e.point[0]),
                        Wide::from_f64_bits(e.point[1]),
                        Wide::from_f64_bits(e.point[2]),
                        Wide::from_u32(e.direction as u32),
                        Wide::from_u32(e.outcome as u32),
                        e.reaction.map_or(Wide::MAX, |r| Wide::from_u32(r.into())),
                    ] {
                        hash = mix(hash, &value.to_le_bytes());
                    }
                    match e.cause {
                        Cause::Saber {
                            weapon,
                            entered_tick,
                            state,
                            attack,
                            edge,
                        } => {
                            let contact = (e.source, entered_tick, attack, e.target);
                            assert!(
                                !saber_targets.contains(&contact),
                                "repeated saber hit at {}",
                                e.tick
                            );
                            saber_targets.push(contact);
                            hash = mix(hash, &entered_tick.to_le_bytes());
                            for v in [weapon, u32::from(state), attack, u32::from(edge)] {
                                hash = mix(hash, &v.to_le_bytes());
                            }
                            if e.before > 0. && e.after == 0. {
                                kills += 1;
                            }
                        }
                        Cause::Bolt {
                            slot,
                            generation,
                            definition,
                        } => {
                            for v in [slot, generation, definition] {
                                hash = mix(hash, &v.to_le_bytes());
                            }
                            if e.outcome == Outcome::Deflected {
                                deflects += 1;
                            }
                            if e.before > 0. && e.after == 0. && Some(e.target.slot) == owner_id {
                                losses += 1;
                                killed_life = Some(e.target);
                            }
                        }
                        Cause::Throw {weapon, launched, returning} => {
                            hash = mix(hash, &weapon.to_le_bytes());
                            hash = mix(hash, &launched.to_le_bytes());
                            hash = mix(hash, &[u8::from(returning)]);
                        }
                        Cause::Shot { definition } | Cause::Explosion { definition } | Cause::Power { definition } => {
                            hash = mix(hash, &definition.to_le_bytes())
                        }
                        Cause::Spawn => {
                            if killed_life
                                .is_some_and(|old| old.slot == e.target.slot && old != e.target)
                            {
                                replacements += 1;
                                killed_life = None;
                            }
                        }
                    }
                    if run == 0 {
                        println!(
                            "fight event {} {} {:?} {:?} {:?} {:?} {} {}",
                            e.tick,
                            e.sequence,
                            e.source,
                            e.target,
                            e.cause,
                            e.outcome,
                            e.before,
                            e.after
                        );
                    }
                }
            }
            if n % 300 == 0 {
                println!("fight digest {run} {n} {hash:016x} inputs={input_hash:016x}");
                if run == 0 {
                    expected.push((hash, input_hash));
                } else {
                    assert_eq!(expected[n / 300 - 1], (hash, input_hash));
                }
            }
        }
        println!("fight totals run={run} saber_kills={kills} redirects={deflects} human_deaths={losses} human_respawns={replacements} host_allocations={total} peak_per_tick={peaks}");
        if run == 0 {
            if let Some(path) = std::env::var_os("BFE_FIGHT_INPUT_OUT") {
                use std::io::Write;
                let mut out = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
                for (held, x, extra) in &script {
                    writeln!(
                        out,
                        "{} {:016x} {} {} {}",
                        held,
                        x.to_bits(),
                        extra.block,
                        extra.melee_held,
                        extra.melee_pressed
                    )
                    .unwrap();
                }
                out.flush().unwrap();
            }
        }
        rt.close();
        assert!(
            kills > 0 && deflects > 0,
            "missing saber kill or bolt deflection"
        );
        assert!(
            losses > 0 && replacements > 0,
            "missing blaster death or subsequent respawn"
        );
    }
}
