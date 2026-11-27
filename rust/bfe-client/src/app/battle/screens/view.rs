//! Join the fixed-tick mission publication, replicated character table and admission metadata.
use crate::{
    local::mission_view::{self as mission, Observation},
    online::{hud::Hud, receiver::Receiver},
};
use bfe_battle_ui::{battle2::*, spawn::view::MapProjection};
use smallvec::SmallVec;

pub(super) struct Names {
    profile: String,
    classes: std::collections::BTreeMap<u16, String>,
}

impl Names {
    pub(super) fn new(
        profile: &str,
        classes: &[crate::online::hud::HudClassMeta],
        catalog: &bfe_render::hud_model2::catalog::Catalog,
    ) -> Self {
        let classes = classes.iter().filter_map(|c| {
            let id = u16::try_from(c.registry_id).ok()?;
            let key = format!("entity.{}", c.class_name.replacen('_', ".", 1));
            Some((id, catalog.localized(&key).unwrap_or(&c.display_name).to_owned()))
        }).collect();
        Self { profile: profile.to_owned(), classes }
    }

    /// The localized name of the class with this registry id.
    pub(super) fn class(&self, id: u16) -> Option<&str> {
        self.classes.get(&id).map(String::as_str)
    }

    fn name<'a>(&'a self, local: u32, actor: mission::Participant, wire: &'a str) -> &'a str {
        if Some(local) == u32::try_from(actor.slot).ok() {
            &self.profile
        } else if actor.human {
            wire
        } else {
            actor.class.and_then(|id| self.classes.get(&id)).map_or("", String::as_str)
        }
    }
}

fn sample(a: f32, b: f32) -> Sample {
    Sample {
        previous: a,
        current: b,
    }
}
fn gauge(a: f32, b: f32, maximum: f32) -> Gauge {
    Gauge {
        value: sample(a, b),
        maximum,
    }
}
fn point(projection: MapProjection, previous: [f32; 2], current: [f32; 2]) -> Point {
    let [a, b] = [previous, current].map(|[x, z]| projection.map_point(x.into(), z.into(), 1.));
    std::array::from_fn(|i| sample((a[i] + 0.5) as f32, (b[i] + 0.5) as f32))
}
fn subject(row: &mission::Body) -> Subject {
    Subject {
        id: row.life.slot,
        generation: row.life.generation,
    }
}

pub(in crate::app::battle) fn phase(
    observed: &Observation,
    receiver: &Receiver,
    hud: &Hud,
) -> Phase {
    use bfe_sim::battle::script::state::CompactOutcome::*;
    if let Some(outcome) = observed.score.outcome {
        let end = match outcome {
            Victory(team @ (1 | 2)) => Outcome::Winner(team as u8),
            Defeat(team @ (1 | 2)) => Outcome::Winner((3 - team) as u8),
            _ => Outcome::Draw,
        };
        return Phase::Ended(end);
    }
    if receiver.own.alive || (hud.own_summary.valid && hud.own_summary.phase == 2) {
        Phase::Playing
    } else {
        Phase::Spawn
    }
}

fn audience(team: Option<i32>) -> Audience {
    team.filter(|t| *t > 0)
        .map_or(Audience::All, |t| Audience::Team(t as u8))
}

fn roster_team(slot: usize, published: u8, local: Option<(usize, u8)>) -> Option<u8> {
    let team = local.filter(|(player, _)| *player == slot).map_or(published, |(_, t)| t);
    matches!(team, 1 | 2).then_some(team)
}

/// Choose each slot's icon key from the observed weapon (`HUDTag`/`HUDModel`/class),
/// falling back to its 3D `GeometryName`/`IconTexture` when the primary key does not
/// resolve to an admitted HUD icon texture.
pub(super) fn slot_icons<'a>(
    observed: &'a Observation,
    catalog: &bfe_render::hud_model2::catalog::Catalog,
    art: &super::ScreenArt,
) -> [&'a str; 2] {
    let bindings = ["player1.weapon1.mesh", "player1.weapon2.mesh"];
    std::array::from_fn(|i| {
        let Some(slot) = observed.slots[i].as_ref() else {
            return "";
        };
        let primary = slot.icon.as_str();
        let fallback = slot.icon_fallback.as_str();
        let mapped = catalog.layout.mesh(bindings[i], primary);
        if art.has_mesh_art(catalog, mapped) || fallback.is_empty() {
            primary
        } else {
            let mapped_fb = catalog.layout.mesh(bindings[i], fallback);
            if art.has_mesh_art(catalog, mapped_fb) {
                fallback
            } else {
                primary
            }
        }
    })
}

/// All references expire after the draw or input callback. Normal mission rosters fit inline.
#[allow(clippy::too_many_arguments)]
pub(super) fn observed<R>(
    observed: &Observation,
    receiver: &Receiver,
    hud: &Hud,
    projection: MapProjection,
    mode: Mode,
    map_name: &str,
    map_image: &str,
    team_labels: &[String; 2],
    names: &Names,
    slot_icons: [&str; 2],
    cockpit: Option<&super::flight::Cockpit<'_>>,
    camera: Option<&crate::scene::View>,
    consume: impl FnOnce(&BattleView<'_>) -> R,
) -> R {
    let own = &hud.own_summary;
    let team = if receiver.own.team != 0 {
        receiver.own.team
    } else {
        own.team
    };
    let local = observed.local_character.or_else(|| receiver.roster.local.map(|slot| (slot, team)));
    let player = local.map_or(u32::MAX, |(slot, _)| slot as u32);
    let phase = phase(observed, receiver, hud);
    let teams = std::array::from_fn(|i| {
        let info = &observed.admission.teams[i];
        Team {
            id: (i + 1) as u8,
            name: Caption::Literal(&team_labels[i]),
            icon: &info.icon,
            score: observed.score.teams[i] as i32,
            target: observed.score.target,
            reinforcements: observed.reinforcements[i],
            show_score: observed.show_points[i],
        }
    });
    let body = observed
        .bodies
        .iter()
        .find(|b| b.human && b.alive)
        .map(|b| {
            let slots = std::array::from_fn(|i| {
                observed.slots[i].as_ref().map(|s| {
                    let kind = match s.base.as_str() {
                        "melee" => SlotKind::Saber,
                        _ if s.power => SlotKind::Power,
                        _ => SlotKind::Weapon,
                    };
                    Slot {
                        kind,
                        name: Caption::Literal(s.name.as_str()),
                        icon: slot_icons[i],
                        rounds: s.rounds,
                        reserve: s.reserve,
                        ready_at: s.ready,
                        changed_at: Some(s.changed),
                        charge: s.charge.map(|p| gauge(p, p, 1.)),
                        clip: s.clip.map(|(a, b)| gauge(a, b, 1.)),
                        heat: s.heat.map(|(a, b)| gauge(a, b, 1.)),
                        overheated: s.overheated,
                        refire: s.refire.map(|(a, b)| gauge(a, b, 1.)),
                    }
                })
            });
            let role = if b.awarded {
                Role::AwardedHero
            } else if mode == Mode::HeroAssault {
                Role::RosterHero
            } else {
                Role::Trooper
            };
            Body {
                subject: subject(b),
                role,
                health: gauge(b.before.health, b.now.health, b.max_health),
                stamina: gauge(b.before.energy, b.now.energy, b.max_energy),
                blocking: b.guarding,
                map: point(projection, b.before.position, b.now.position),
                yaw: sample(b.before.yaw, b.now.yaw),
                reticle: observed.slots[0]
                    .as_ref()
                    .map_or("", |s| s.reticle.as_str()),
                slots,
            }
        });
    let contacts: SmallVec<[_; 64]> = observed
        .bodies
        .iter()
        .filter(|b| !observed.map_hidden && b.alive && !b.human && b.team == team)
        .map(|b| Contact {
            subject: subject(b),
            team: b.team,
            map: point(projection, b.before.position, b.now.position),
            icon: "",
        })
        // The mission's command posts ride the same list, under their own icon name.
        .chain(
            hud.post_meta
                .iter()
                .filter(|m| m.active && !observed.map_hidden)
                .map(|m| {
                    let at = [m.position[0] as f32, m.position[2] as f32];
                    Contact {
                        subject: Subject { id: m.id as u32, generation: 0 },
                        team: m.team,
                        map: point(projection, at, at),
                        icon: POST_CONTACT,
                    }
                }),
        )
        .collect();
    let objectives: SmallVec<[_; 16]> = observed
        .objectives
        .iter()
        .map(|o| Objective {
            id: o.id,
            audience: audience(Some(o.team)),
            text: Caption::Key(o.key.as_str()),
            complete: o.complete,
        })
        .collect();
    let systems: SmallVec<[System<'_>; 8]> = observed
        .systems
        .iter()
        .filter(|s| s.state.marker)
        .map(|s| {
            let [health, most] = s.state.health.unwrap_or([f32::from(s.state.alive), 1.]);
            System {
                subject: Subject { id: s.state.index.into(), generation: 0 },
                team: s.state.team,
                name: Caption::Literal(s.name.as_str()),
                icon: "",
                hull: gauge(health, health, most),
                shield: s.state.shield.map(|[charge, capacity]| gauge(charge, charge, capacity)),
                destroyed: !s.state.alive,
                // Where the mission puts the mark on its ship picture, as replicated.
                hud: s.state.hud.unwrap_or([0.; 2]),
                marker: s.state.marker,
                ship: "",
                shield_art: "",
            }
        })
        .collect();
    let timer = observed.timer.as_ref().map(|(key, due)| Timer {
        name: Caption::Key(key.as_str()),
        audience: Audience::All,
        due: *due,
    });
    let rows: SmallVec<[_; 64]> = receiver
        .roster
        .rows()
        .filter_map(|(i, r)| Some((i, r, r.name(), roster_team(i, r.team, local)?)))
        .collect();
    let mut scores: SmallVec<[_; 64]> = rows
        .iter()
        .map(|(i, r, name, team)| ScoreRow {
            player: *i as u32,
            team: *team,
            name: names.name(player, observed.roster.get(*i).copied().flatten()
                .unwrap_or(mission::Participant { slot: *i, human: r.human, class: None }), name),
            score: observed
                .personal
                .iter()
                .find(|(id, _)| id == i)
                .map_or(0, |(_, p)| *p),
            kills: r.kills.into(),
            deaths: r.deaths.into(),
            assists: 0,
            ping: r.ping,
        })
        .collect();
    if let Some(i) = scores.iter().position(|r| r.player == player) {
        scores[..=i].rotate_right(1);
    }
    let name = |actor: mission::Participant| {
        let wire = rows.iter().find(|r| r.0 == actor.slot).map_or("", |r| r.2.as_ref());
        names.name(player, actor, wire)
    };
    let events: SmallVec<[_; 32]> = observed
        .notices
        .iter()
        .filter_map(|n| {
            let (audience, message) = match &n.value {
                mission::Notice::Message { team, key } => {
                    (audience(*team), Message::Notice(Caption::Key(key.as_str())))
                }
                // A voice-over is heard, not shown.
                mission::Notice::Voice { .. } => return None,
                mission::Notice::Kill {
                    killer,
                    victim,
                    weapon,
                } => {
                    let victim = name(*victim);
                    let killer = killer.map(name).unwrap_or("");
                    let weapon = weapon.as_str();
                    (
                        Audience::All,
                        Message::Kill {
                            killer,
                            victim,
                            weapon,
                            weapon_name: None,
                        },
                    )
                }
            };
            Some(Event {
                sequence: n.sequence,
                until_tick: n.until,
                audience,
                message,
            })
        })
        .collect();
    let target = (phase == Phase::Playing && body.is_some())
        .then_some(observed.locked)
        .flatten()
        .map(|l| {
            let label = l
                .participant
                .map(name)
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    l.class
                        .and_then(|id| names.classes.get(&id))
                        .map(String::as_str)
                })
                .unwrap_or("");
            let current = camera.and_then(|c| super::flight::bracket(c, l.now.center, l.now.radius));
            let previous =
                camera.and_then(|c| super::flight::bracket(c, l.before.center, l.before.radius));
            Target {
                subject: Subject {
                    id: l.life.slot,
                    generation: l.life.generation,
                },
                team: l.team,
                name: Caption::Literal(label),
                health: Some(gauge(l.before.health, l.now.health, l.max_health)),
                lock: Lock::Locked,
                progress: Gauge::default(),
                bracket: current.map(|c| {
                    std::array::from_fn(|i| Sample {
                        previous: previous.unwrap_or(c)[i],
                        current: c[i],
                    })
                }),
            }
        })
        .or_else(|| {
            if phase != Phase::Playing || body.is_none() {
                return None;
            }
            let h = hud.hover_target?;
            if h.relation == 4. {
                return None;
            }
            let label = names
                .classes
                .get(&(h.source as u16))
                .map(String::as_str)
                .unwrap_or("");
            let max_health = hud.max_health_of(h.source).max(1.) as f32;
            Some(Target {
                subject: Subject {
                    id: h.key.0,
                    generation: h.key.1,
                },
                team: h.team,
                name: Caption::Literal(label),
                health: Some(gauge(h.health as f32, h.health as f32, max_health)),
                lock: Lock::None,
                progress: Gauge::default(),
                bracket: None,
            })
        });
    let aim = Aim {
        position: Some([hud.reticle_at[0] as f32, hud.reticle_at[1] as f32]),
        visible: hud.reticle_visible,
        relation: match hud.reticle_relation as u32 {
            1 => AimRelation::Friendly,
            2 | 3 => AimRelation::Enemy,
            0 => AimRelation::Neutral,
            _ => AimRelation::None,
        },
        hit: observed.hit_marker.map(|(tick, critical)| HitMarker { tick, critical }),
        scope: hud.active_scope(),
    };
    let view = BattleView {
        tick: observed.tick,
        phase,
        mode,
        player,
        team,
        map_name: Caption::Key(map_name),
        map_image: if observed.map_hidden { "" } else { map_image },
        teams,
        body,
        vehicle: None,
        aim,
        target,
        contacts: &contacts,
        systems: &systems,
        objectives: &objectives,
        timers: timer.as_slice(),
        events: &events,
        scores: &scores,
        leaving: observed.leaving,
    };
    let mounted = cockpit.filter(|_| phase == Phase::Playing);
    consume(&mounted.map_or(view, |c| c.view(view)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlinked_local_score_keeps_its_team_without_a_body() {
        let local = Some((19, 2));
        assert_eq!(roster_team(19, 0, local), Some(2));
        assert_eq!(roster_team(4, 0, local), None);
        assert_eq!(roster_team(4, 1, local), Some(1));
        assert_eq!(roster_team(19, 0, None), None);
    }

    #[test]
    fn feed_keeps_the_class_at_death_and_players_keep_their_names() {
        let names = Names { profile: "Juniper".into(), classes: [(3, "Birch".into()), (9, "Elm".into())].into() };
        let at_death = mission::Participant { slot: 4, human: false, class: Some(3) };
        let respawn = mission::Participant { class: Some(9), ..at_death };
        let guest = mission::Participant { human: true, ..at_death };
        assert_eq!(names.name(7, at_death, "LOCAL"), "Birch");
        assert_eq!(names.name(7, respawn, "LOCAL"), "Elm");
        assert_eq!(names.name(7, guest, "Guest"), "Guest");
        assert_eq!(names.name(4, guest, "LOCAL"), "Juniper");
    }

    #[test]
    fn slot_icons_preserve_hudtransforms_and_fall_back_to_geometry_mesh() {
        use bfe_render::hud_model2::{catalog::Catalog, Layout};
        let layout = Layout::read(
            br#"{"file":"1playerhud","entries":[]}"#.as_slice(),
            br#"{"file":"hudtransforms","entries":[{"name":"TransformNameMesh","values":["w2"],"children":[
                {"name":"EventOutput","values":["player1.weapon2.mesh"]},
                {"name":"NameMesh","values":["all_weap_inf_thermaldetonator","hud_thermaldetonator"]}
            ]}]}"#
                .as_slice(),
        )
        .unwrap();
        let mut catalog = Catalog::default();
        catalog.layout = layout;
        catalog
            .meshes
            .insert("imp_weap_inf_rifle".into(), "mesh.imp_weap_inf_rifle".into());
        catalog.meshes.insert(
            "hud_thermaldetonator".into(),
            "mesh.hud_thermaldetonator".into(),
        );
        let fonts = std::array::from_fn(|_| bfe_render::interface::Font {
            height: 12.,
            glyphs: Default::default(),
        });
        let mut art = super::super::ScreenArt::load(&Catalog::default(), fonts, 1, |_| unreachable!()).unwrap();
        art.insert("mesh.imp_weap_inf_rifle".into(), (10, 128, 55));
        art.insert("mesh.hud_thermaldetonator".into(), (11, 64, 64));

        assert!(art.has_mesh_art(&catalog, "imp_weap_inf_rifle"));
        assert!(art.has_mesh_art(&catalog, "IMP_WEAP_INF_RIFLE"));
        assert!(!art.has_mesh_art(&catalog, "imp_weap_hero_bobarifle"));
        assert_eq!(
            catalog
                .layout
                .mesh("player1.weapon2.mesh", "all_weap_inf_thermaldetonator"),
            "hud_thermaldetonator"
        );
        assert!(art.has_mesh_art(&catalog, "hud_thermaldetonator"));
    }
}
