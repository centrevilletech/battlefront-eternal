//! Mounted observations extend the same mission view used for soldier, deployment and result screens.
use crate::{local::space::{instruments::{ContactName, Label, Mount}, History, Pose}, scene::View};
use bfe_battle_ui::battle2::{BattleView, Caption, Contact, Gauge, Lock, MapPose, Sample, Seat, Slot, SlotKind, Subject, System, Target, Vehicle};
use std::{collections::BTreeMap, fs::File, io::Read, path::Path};
use three::{Matrix4, Vector3};

#[derive(Default)]
pub(super) struct FlightMap {
    pub(super) chart_extent: Option<([f64; 2], [f64; 2])>,
    pub(super) symbols: BTreeMap<u32, String>,
}

pub(super) struct Cockpit<'a> {
    pub(super) fraction: f32,
    /// The seat's instruments, or none on foot (the critical systems show either way).
    vehicle: Option<Vehicle<'a>>,
    target: Option<Target<'a>>,
    contacts: arrayvec::ArrayVec<Contact<'a>, { bfe_sim::space::MAX_PREPARED_FIGHTERS }>,
    systems: arrayvec::ArrayVec<System<'a>, { bfe_sim::battle::script::state::MAX_CRITICAL_SYSTEMS }>,
    /// The craft the player sits in, or the rocket he steers from it, is outside the
    /// battlefield (its out-of-bounds timer runs): the game shows the same warning for both.
    leaving: bool,
}

impl Cockpit<'_> {
    pub(super) fn view<'a>(&'a self, mission: BattleView<'a>) -> BattleView<'a> {
        match self.vehicle {
            Some(vehicle) => BattleView {
                body: None,
                vehicle: Some(vehicle),
                target: self.target,
                contacts: &self.contacts,
                systems: if self.systems.is_empty() { mission.systems } else { &self.systems },
                leaving: mission.leaving || self.leaving,
                ..mission
            },
            None => BattleView {
                systems: if self.systems.is_empty() { mission.systems } else { &self.systems },
                leaving: mission.leaving || self.leaving,
                ..mission
            },
        }
    }
}

impl FlightMap {
    pub(super) fn sample<'a>(&'a self, history: &'a History, clock: f64, camera: Option<&View>, catalog: &bfe_render::hud_model2::catalog::Catalog) -> Option<Cockpit<'a>> {
        let (frame, alpha) = history.sample(clock)?;
        let names = frame.labels.as_ref()?;
        // An online battle's names may hold no mission HUD: then no systems strip.
        let previous = history.previous(frame.tick);
        let systems = names.mission.get().map(|hud| assault_systems(frame, previous, hud, catalog)).unwrap_or_default();
        let Some(now) = frame.cockpit else {
            return Some(Cockpit {
                fraction: alpha as f32,
                vehicle: None,
                target: None,
                contacts: arrayvec::ArrayVec::new(),
                systems,
                leaving: frame.out_of_bounds.is_some() || frame.rocket_out_of_bounds.is_some(),
            });
        };
        let old = previous
            .and_then(|f| f.cockpit)
            .filter(|c| {
                (c.mount, c.slot, c.generation, c.seat)
                    == (now.mount, now.slot, now.generation, now.seat)
            })
            .unwrap_or(now);
        let subject = Subject {
            id: now.slot as u32,
            generation: now.generation,
        };
        let [x, z, half] = frame.radar;
        let (center, half) = self.chart_extent.unwrap_or(([x, z], [half; 2]));
        let map = |p: Pose| {
            let xz = [p.position[0], p.position[2]];
            std::array::from_fn::<_, 2, _>(|i| (0.5 + (xz[i] - center[i]) / (2. * half[i])) as f32)
        };
        let heading = |p: Pose| {
            let [x, y, z, w] = p.rotation;
            (2. * (x * z + w * y)).atan2(1. - 2. * (x * x + y * y)) as f32
        };
        // The authority bounds the admitted fleet. Borrow symbol names for this draw.
        let mut contacts = arrayvec::ArrayVec::<_, { bfe_sim::space::MAX_PREPARED_FIGHTERS }>::new();
        let mut map_pose = None;
        for hull in &frame.hulls {
            let previous = map(hull.previous);
            let current = map(hull.current);
            let map = std::array::from_fn(|i| Sample {
                previous: previous[i],
                current: current[i],
            });
            let id = Subject {
                id: hull.id as u32,
                generation: hull.generation,
            };
            if matches!(now.mount, Mount::Fighter(_)) && id == subject {
                map_pose = Some(MapPose {
                    map,
                    yaw: Sample {
                        previous: heading(hull.previous),
                        current: heading(hull.current),
                    },
                });
            } else {
                contacts.push(Contact {
                    subject: id,
                    team: hull.team,
                    map,
                    icon: self.symbols.get(&hull.class).map_or("", String::as_str),
                });
            }
        }
        let slots = std::array::from_fn(|channel| {
            let w = now.weapons[channel]?;
            let equipment = match now.mount {
                Mount::Fighter(profile) => {
                    names.weapons.get(usize::try_from(profile).ok()?)?.get(w.index)?
                }
                Mount::Turret(building) => names
                    .turret_weapons
                    .get(usize::try_from(building).ok()?)?
                    .as_ref()?,
            };
            let prev = old.weapons[channel].filter(|p| p.index == w.index);
            let clip = w.clip.map(|(v, max)| {
                gauge(prev.and_then(|p| p.clip).map_or(v, |(p, _)| p), v, max)
            });
            let heat = w.heat.map(|(v, max)| {
                gauge(prev.and_then(|p| p.heat).map_or(v, |(p, _)| p), v, max)
            });
            let refire = w
                .refire
                .map(|v| gauge(prev.and_then(|p| p.refire).unwrap_or(v), v, 1.));
            Some(Slot {
                kind: SlotKind::Weapon,
                name: caption(&equipment.label, catalog),
                icon: &equipment.icon,
                rounds: w.rounds,
                reserve: None,
                changed_at: frame.weapon_changed[channel],
                ready_at: None,
                charge: None,
                clip,
                heat,
                overheated: w.overheated,
                refire,
            })
        });
        let target = now.target.map(|t| {
            let prev = old
                .target
                .filter(|p| (p.id, p.life, p.name) == (t.id, t.life, t.name))
                .unwrap_or(t);
            let label = match t.name {
                ContactName::Craft(i) => names.craft.get(i),
                ContactName::Building(i) => names.objects.get(i),
            }
            .map(|s| caption(s, catalog))
            .unwrap_or(Caption::Literal(""));
            let current = camera.and_then(|c| bracket(c, t.position, t.radius));
            let previous = camera.and_then(|c| bracket(c, prev.position, prev.radius));
            Target {
                subject: Subject {
                    id: t.id,
                    generation: t.life,
                },
                team: t.team,
                name: label,
                health: Some(gauge(prev.hp, t.hp, t.max_hp)),
                lock: if t.locked {
                    Lock::Locked
                } else {
                    Lock::Acquiring
                },
                progress: gauge(prev.fraction, t.fraction, 1.),
                bracket: current.map(|c| {
                    std::array::from_fn(|i| Sample {
                        previous: previous.unwrap_or(c)[i],
                        current: c[i],
                    })
                }),
            }
        });
        let vehicle = Vehicle {
            subject,
            seat: if now.seat == 0 {
                Seat::Pilot
            } else {
                Seat::Gunner
            },
            map_pose,
            hull: gauge(old.hull, now.hull, now.hull_max),
            speed: gauge(old.speed, now.speed, now.speed_max),
            boost: (now.energy_max > 0.).then(|| gauge(old.energy, now.energy, now.energy_max)),
            slots,
            landing: now.can_land.then_some(Caption::Key("ifs.controls.General.Land_Takeoff")),
        };
        Some(Cockpit { fraction: alpha as f32, vehicle: Some(vehicle), target, contacts, systems, leaving: frame.out_of_bounds.is_some() || frame.rocket_out_of_bounds.is_some() })
    }
}

/// Only the active map can resolve a class bitmap. Older preparations omit this catalog.
pub(super) fn radar_symbols(
    root: &Path,
    map_id: &str,
    classes: &BTreeMap<u32, String>,
    mut upload: impl FnMut(&str, &Path) -> Result<(), String>,
) -> Result<BTreeMap<u32, String>, String> {
    use bfe_render::hud_model2::catalog::child;
    #[derive(serde::Deserialize)]
    struct Index {
        textures: BTreeMap<String, String>,
    }
    let name = format!("maps/{map_id}/hud/catalog.json");
    if !root.join(&name).try_exists().map_err(|e| e.to_string())? {
        return Ok(BTreeMap::new());
    }
    let catalog = child(root, &name)?;
    let folder = catalog.parent().ok_or("missing map catalog directory")?;
    let mut bytes = Vec::new();
    File::open(&catalog)
        .and_then(|f| f.take((1 << 20) + 1).read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    if bytes.len() > 1 << 20 {
        return Err("oversized map symbol catalog".into());
    }
    let index: Index = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let mut symbols = BTreeMap::new();
    let mut loaded = std::collections::BTreeSet::new();
    for (&class, key) in classes {
        let Some(file) = index.textures.get(key) else { continue };
        if loaded.insert(key) {
            if Path::new(file).file_name() != Some(std::ffi::OsStr::new(file)) {
                return Err("expected a map symbol basename".into());
            }
            upload(key, &child(folder, file)?)?;
        }
        symbols.insert(class, key.to_owned());
    }
    Ok(symbols)
}

fn caption<'a>(
    label: &'a Label,
    catalog: &bfe_render::hud_model2::catalog::Catalog,
) -> Caption<'a> {
    if catalog.localized(&label.key).is_some() {
        Caption::Key(&label.key)
    } else {
        Caption::Literal(&label.fallback)
    }
}

fn gauge(previous: f32, current: f32, maximum: f32) -> Gauge {
    Gauge {
        value: Sample { previous, current },
        maximum,
    }
}

pub(super) fn bracket(camera: &View, [x, y, z]: [f64; 3], radius: f64) -> Option<[f32; 4]> {
    let mut m = Matrix4::default();
    m.elements.copy_from_slice(&camera.values[16..32]);
    let mut p = Vector3::new(x, y, z);
    p.apply_matrix4(&m);
    if p.z >= 0. || radius <= 0. {
        return None;
    }
    let depth = -p.z;
    m.elements.copy_from_slice(&camera.values[..16]);
    p.apply_matrix4(&m);
    let hw = 0.5 * radius * m.elements[0] / depth;
    let hh = 0.5 * radius * m.elements[5] / depth;
    let (cx, cy) = ((1. + p.x) * 0.5, (1. - p.y) * 0.5);
    Some([cx - hw, cy - hh, cx + hw, cy + hh].map(|v| v as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radar_uses_only_the_selected_maps_symbols_and_shares_class_art() {
        let root = std::env::temp_dir().join(format!("bfe-flight-symbols-{}", std::process::id()));
        let folder = root.join("maps/garden/hud");
        std::fs::create_dir_all(&folder).unwrap();
        let root = root.canonicalize().unwrap();
        std::fs::write(folder.join("leaf.png"), []).unwrap();
        std::fs::write(folder.join("catalog.json"), br#"{"textures":{"sprout":"leaf.png"}}"#).unwrap();
        let classes = [(4, "sprout".into()), (9, "absent".into()), (12, "sprout".into())].into();
        let mut loaded = Vec::new();
        let found = radar_symbols(&root, "garden", &classes, |name, file| {
            loaded.push((name.to_owned(), file.to_owned()));
            Ok(())
        }).unwrap();
        assert_eq!(loaded, [("sprout".into(), folder.join("leaf.png").canonicalize().unwrap())]);
        assert_eq!(found.keys().copied().collect::<Vec<_>>(), [4, 12]);
        assert!(radar_symbols(&root, "meadow", &classes, |_, _| unreachable!()).unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn target_bounds_follow_camera_depth_and_exclude_objects_behind_it() {
        let mut camera = View {
            values: [0.; bfe_render::shader::FRAME_FLOATS],
        };
        camera.values[16..32].copy_from_slice(&Matrix4::default().elements);
        camera.values[0] = 1.;
        camera.values[5] = 1.;
        camera.values[11] = -1.;
        assert_eq!(
            bracket(&camera, [0., 0., -4.], 2.),
            Some([0.25, 0.25, 0.75, 0.75])
        );
        assert_eq!(bracket(&camera, [0., 0., 4.], 2.), None);
    }
}

/// The mission's critical systems in the order it registered them, each with its mark's place
/// and its team's ship and shield pictures. A linked set's first system shows the set's health.
fn assault_systems<'a>(
    frame: &'a crate::local::space::Frame,
    previous: Option<&'a crate::local::space::Frame>,
    hud: &'a crate::local::space::instruments::MissionHud,
    catalog: &bfe_render::hud_model2::catalog::Catalog,
) -> arrayvec::ArrayVec<System<'a>, { bfe_sim::battle::script::state::MAX_CRITICAL_SYSTEMS }> {
    let mut systems = arrayvec::ArrayVec::new();
    for registered in &hud.systems {
        let Some(s) = frame.system(registered.building) else { continue };
        let prior = |f: Option<&'a crate::local::space::Frame>, building: usize, now: &crate::local::space::System| {
            f.and_then(|f| f.system(building)).filter(|p| p.generation == now.generation).map_or((now.health, now.max_health), |p| (p.health, p.max_health))
        };
        let (mut health, mut max) = (s.health, s.max_health);
        let (mut before, mut before_max) = prior(previous, s.building, s);
        for &part in &registered.linked {
            if let Some(l) = frame.system(part) {
                health += l.health;
                max += l.max_health;
                let (h, m) = prior(previous, part, l);
                before += h;
                before_max += m;
            }
        }
        let old_shield = previous
            .and_then(|f| f.system(s.building))
            .filter(|p| p.generation == s.generation)
            .map_or(s.shield, |p| p.shield);
        let team = hud.teams.iter().find(|t| t.id == s.team);
        let art = |pick: fn(&crate::local::space::instruments::TeamNames) -> &str| team.map_or("", pick);
        if systems
            .try_push(System {
                subject: Subject { id: s.building as u32, generation: s.generation },
                team: s.team,
                name: caption(&registered.label, catalog),
                icon: art(|t| t.system_icon.as_str()),
                // One maximum for both samples keeps the drawn fraction continuous.
                hull: gauge(
                    (before / before_max.max(1e-9) * max) as f32,
                    health as f32,
                    max as f32,
                ),
                shield: (s.max_shield > 0.)
                    .then(|| gauge(old_shield as f32, s.shield as f32, s.max_shield as f32)),
                destroyed: !s.alive,
                hud: registered.hud,
                marker: registered.show_marker,
                ship: art(|t| t.ship_icon.as_str()),
                shield_art: art(|t| t.shield_icon.as_str()),
            })
            .is_err()
        {
            break;
        }
    }
    systems
}
