//! Seat commands used by authority hosts. F2Seats.invoke in
//! src/sim/wasm/f2-seats.js passes scalar requests to the same native owner;
//! the owner validates both the soldier life and the current occupancy.
use crate::abi::context::Context;
use crate::gen::{
    self, F2SeatCommand, F2SeatOp, F2SeatResult, SeatLease, SeatReason, Unit, Vehicle,
};
use crate::store::Ref;

/// The host supplies canonical identities, never a mirror's pointer or index.
/// `Man` uses the authored turret index; a hull request names its store row.
#[derive(Clone, Copy, Debug)]
pub enum Request {
    Turret(usize, f64),
    Leave,
    Hull(Ref<Vehicle>, f64, usize),
}

/// F2Seats.invoke defaults placement to true, direction to one, and reason
/// to exit. In particular, turretLeave carries the existing lease and does
/// not substitute the destination turret index for its default zero index.
pub fn command(
    unit: Ref<Unit>,
    life: f64,
    lease: Option<Ref<SeatLease>>,
    request: Request,
) -> F2SeatCommand {
    // F2Seats.invoke sends the soldier's gameplay generation separately
    // from its slab generation. Reusing either identity for the other lets
    // a delayed board request act on a respawned soldier.
    // No foreign control row is needed: native players and seat controls
    // already share their canonical intent with the fleet owner.
    let mut out = F2SeatCommand {
        unit: Some(unit),
        unit_life: life,
        placement: true,
        reason: SeatReason::Exit,
        direction: 1.0,
        ..Default::default()
    };
    // The JavaScript wrappers choose different argument shapes for these
    // three calls. In particular, man has no hull, and leave supplies the
    // current lease rather than asking the turret index to identify it.
    match request {
        Request::Turret(index, life) => {
            out.hull_life = life;
            out.op = F2SeatOp::Man;
            out.index = index as f64;
        }
        Request::Leave => {
            out.op = F2SeatOp::TurretLeave;
            out.lease = lease;
        }
        Request::Hull(hull, life, index) => {
            out.op = F2SeatOp::Board;
            out.hull = Some(hull);
            out.hull_life = life;
            out.index = index as f64;
        }
    }
    out
}

/// The generated command boundary also performs the external publication
/// barrier F3Sim.beforeCommand/afterCommand requires. The caller drains its
/// resulting notices before the next input, while the old clock still holds.
pub fn apply(context: &Context, command: &F2SeatCommand) -> Result<bool, String> {
    // The generated F2 seat operation dispatches the same owning command
    // as the module's cmd_f2_seat_block export. sim_command deliberately
    // refuses this command because it cannot lend the fleet owner.
    // The operation's generated row has no heap and carries no scalar
    // prefix. Both soldier and hull identities remain generation-qualified.
    let mut row = [0.0; F2SeatCommand::WORDS];
    command.put(&mut row, &mut Vec::new());
    let block = context
        .run_operation(gen::OPERATION_F2_SEAT, &row)
        .map_err(|e| format!("seat operation: {e:?}"))?;
    let at = block.words[gen::BlockHeader::HEAP] as usize;
    Ok(F2SeatResult::take(&block.words[gen::BlockHeader::WORDS..], &block.words[at..]).accepted)
}

/// Values sampled by Game.mountedShot, separate from the weapon registry's
/// immutable name and channel. Later exits must not change the source life.
pub struct Shot<'a> {
    pub sample: &'a gen::F3MountedSample,
    pub seat: f64,
    pub position: [f64; 3],
    pub direction: [f64; 3],
    pub weapon: Option<&'a str>,
    pub weapon_id: f64,
    pub channel: f64,
    pub fire_id: f64,
}

/// Game.mountedShot emits a journal shot and attachPresentationCapture emits
/// its wire presentation. Mounted weapons never set infantry repair, melee,
/// or swing fields; their source and mount are emission-time samples.
pub fn fired(
    context: &Context,
    journal: &mut super::journal::Journal,
    capture: Option<&super::capture::Capture>,
    tick: f64,
    shot: &Shot<'_>,
) -> Result<(), String> {
    use super::capture::Token;
    use super::journal::{Event, Life};
    let s = shot.sample;
    let weapon = shot
        .weapon
        .map(|name| context.intern(&name.encode_utf16().collect::<Vec<_>>()))
        .transpose()
        .map_err(|e| e.message().to_string())?;
    journal.emit(Event::MountedShot {
        source: s.has_source.then_some(Life {
            id: s.source.id as u32,
            generation: s.source.generation,
        }),
        mount: Life {
            id: s.mount.id as u32,
            generation: s.mount.generation,
        },
        seat: shot.seat,
        team: s.team,
        position: shot.position,
        direction: shot.direction,
        weapon,
        repair: false,
        melee: false,
        swing: 0.0,
    });
    // A host without a network capture still needs its gameplay journal.
    // Captured hosts append a cosmetic wire record after the journal emit,
    // preserving ordering against other shot and ordnance notifications.
    if let Some(capture) = capture {
        capture.emit(
            context,
            tick,
            "shot",
            0.0,
            &[
                (
                    "ownerId",
                    Token::Number(if s.has_source { s.source.id } else { 0.0 }),
                ),
                (
                    "ownerGeneration",
                    Token::Number(if s.has_source {
                        s.source.generation
                    } else {
                        0.0
                    }),
                ),
                ("mountId", Token::Number(s.mount.id)),
                ("mountGeneration", Token::Number(s.mount.generation)),
                ("seat", Token::Number(shot.seat)),
                ("fireId", Token::Number(shot.fire_id)),
                ("channel", Token::Number(shot.channel)),
                ("weaponId", Token::Number(shot.weapon_id)),
                ("position", Token::Vector(Some(shot.position))),
                ("repair", Token::Boolean(false)),
                ("melee", Token::Boolean(false)),
                ("swing", Token::Number(0.0)),
            ],
        )?;
    }
    Ok(())
}

/// The ordered fleet notices share the ordinary unit journal and capture.
/// Press ids come from the host's player mirror. Game.beforeWorldFinish
/// calls multiplayer-world.js mirrorShots before delivering shot notices.
pub struct Notifications<'a> {
    pub context: &'a Context,
    pub journal: &'a mut super::journal::Journal,
    pub capture: Option<&'a super::capture::Capture>,
    pub tick: f64,
    pub store_source: u32,
    pub presses: &'a [(u32, [Option<f64>; 2])],
}

impl Notifications<'_> {
    /// F3Sim.notifications dispatches fleet shots through Game.mountedShot;
    /// bind and sound turret notices have no SimEvents journal listener.
    pub fn handle(&mut self, code: u32, record: &[f64]) -> Result<bool, String> {
        // F2 fleet shots and stationary turret notices reach the same
        // Game callback through different generated event layouts.
        // Normalize only the callback arguments, keeping their event-time
        // position, direction and source/mount samples untouched.
        let (sample, seat, position, direction, definition) = match code {
            gen::EVENT_F3_TURRET if record.len() >= gen::F2TurretEvent::WORDS => {
                let e = gen::F2TurretEvent::take(
                    &record[..gen::F2TurretEvent::WORDS],
                    &record[gen::F2TurretEvent::WORDS..],
                );
                if e.kind != gen::F2TurretEventKind::Fired {
                    return Ok(true);
                }
                let definition = self
                    .context
                    .turret_weapon_definition(e.turret, e.sample.mount.generation)
                    .map_err(|e| e.message().to_string())?;
                (e.sample, 0.0, e.origin, e.direction, definition)
            }
            _ => return Ok(false),
        };
        // WeaponFacade.name and channel are immutable class properties.
        // Read the base level, rather than a charge level's current state:
        // the page's presentation registry identifies the weapon class.
        // A missing class stays null in the journal and zero on the wire.
        let levels = definition
            .map(|id| self.context.weapon_levels(id))
            .transpose()
            .map_err(|e| e.message().to_string())?;
        let first = levels.as_ref().and_then(|l| l.first());
        let name = first
            .and_then(|l| l.class.as_ref())
            .map(|name| String::from_utf16_lossy(name));
        let channel = first.map_or(0.0, |l| l.channel);
        // Native content can include classes the mission never registers.
        // The source's protocol map owns the wire identity; its definition
        // index must never be used directly as a network weapon id.
        let weapon_id = match name.as_deref() {
            Some(name) if self.capture.is_some() => {
                let id = self
                    .context
                    .intern(&name.encode_utf16().collect::<Vec<_>>())
                    .map_err(|e| e.message().to_string())?;
                self.context
                    .wire_weapon_id(self.store_source, id)
                    .map_err(|e| e.message().to_string())?
            }
            _ => 0.0,
        };
        // Network presses are selected by channel. Secondary fire keeps
        // its own prediction identity, while bots have no press record.
        // The caller supplies the published player mirror; choosing its
        // update point belongs to the enclosing authority tick.
        let fire_id = self
            .presses
            .iter()
            .find(|(unit, _)| sample.has_source && f64::from(*unit) == sample.source.id)
            .and_then(|(_, presses)| presses[usize::from(channel == 1.0)])
            .unwrap_or(0.0);
        fired(
            self.context,
            self.journal,
            self.capture,
            self.tick,
            &Shot {
                sample: &sample,
                seat,
                position,
                direction,
                weapon: name.as_deref(),
                weapon_id,
                channel,
                fire_id,
            },
        )?;
        Ok(true)
    }
}

impl Context {
    /// F3Sim.afterCommand drains synchronous seat notices before the next
    /// ordinary player input. `next_tick` names the step that follows it;
    /// the command and its notices still use that step's previous clock.
    pub fn world_host_mount(
        &self,
        command: &F2SeatCommand,
        next_tick: f64,
    ) -> Result<bool, crate::abi::context::Error> {
        let accepted = apply(self, command).map_err(crate::abi::context::Error::host)?;
        self.host(|host| host.acknowledge(self, next_tick, usize::MAX))?
            .map_err(crate::abi::context::Error::host)?;
        Ok(accepted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leave_preserves_the_occupancy_and_invoke_defaults() {
        // A turret exit has no hull identity and retains its exact lease.
        // The zero default index must not accidentally select turret zero:
        // the native owner resolves the turret through this lease first.
        let lease = Ref::new(7, 3);
        let out = command(Ref::new(2, 1), 4.0, Some(lease), Request::Leave);
        assert_eq!(out.op, F2SeatOp::TurretLeave);
        assert_eq!(out.lease, Some(lease));
        assert_eq!(out.index, 0.0);
        assert!(out.placement);
        assert_eq!(out.direction, 1.0);
        assert_eq!(out.reason, SeatReason::Exit);
    }

    #[test]
    fn boarding_qualifies_hull_and_soldier_lives() {
        // Distinct slab and gameplay generations make accidental identity
        // substitution visible without requiring authored game content.
        // Explicit seat selection is preserved, including non-driver seats.
        let out = command(
            Ref::new(2, 1),
            4.0,
            None,
            Request::Hull(Ref::new(9, 2), 8.0, 1),
        );
        assert_eq!(out.op, F2SeatOp::Board);
        assert_eq!(out.unit_life, 4.0);
        assert_eq!(out.hull_life, 8.0);
        assert_eq!(out.index, 1.0);
        assert!(out.lease.is_none());
        let turret = command(Ref::new(2, 1), 4.0, None, Request::Turret(6, 3.0));
        assert_eq!(turret.op, F2SeatOp::Man);
        assert_eq!(turret.index, 6.0);
        assert!(turret.hull.is_none());
    }
}

#[cfg(test)]
mod shot_tests {
    use super::*;

    #[test]
    fn mounted_shot_retains_sampled_lives_and_hud_fields() {
        // Source life, mount life, holder seat and team are independent.
        // The typed record must preserve each sampled gameplay identity.
        // No live unit is created, proving the record uses its saved sample.
        // The source assertion checks the identity the AI's hearing reads.
        let context = Context::new();
        let mut journal = super::super::journal::Journal::new();
        let sample = gen::F3MountedSample {
            has_source: true,
            source: gen::F3LifeRef {
                id: 3.0,
                generation: 4.0,
            },
            mount: gen::F3LifeRef {
                id: 70.0,
                generation: 2.0,
            },
            team: 1.0,
        };
        let shot = Shot {
            sample: &sample,
            seat: 1.0,
            position: [1.0, 2.0, 3.0],
            direction: [0.0, 0.0, 1.0],
            weapon: Some("laser"),
            weapon_id: 9.0,
            channel: 1.0,
            fire_id: 2.0,
        };
        fired(&context, &mut journal, None, 300.0, &shot).unwrap();
        let record = journal.records().next().unwrap();
        assert!(matches!(
            record.event,
            super::super::journal::Event::MountedShot {
                seat: 1.0,
                mount: super::super::journal::Life {
                    id: 70,
                    generation: 2.0
                },
                ..
            }
        ));
        assert!(matches!(
            record.event,
            super::super::journal::Event::MountedShot {
                source: Some(super::super::journal::Life {
                    id: 3,
                    generation: 4.0
                }),
                ..
            }
        ));
    }
}

#[cfg(test)]
mod notification_tests {
    use super::*;

    #[test]
    fn turret_sound_notice_has_no_journal_listener() {
        // F2Seats.turretNotice routes this to the page audio hook. The
        // SimEvents listener observes only its distinct fired notification.
        let context = Context::new();
        let mut journal = super::super::journal::Journal::new();
        let mut row = vec![0.0; gen::F2TurretEvent::WORDS];
        gen::F2TurretEvent {
            kind: gen::F2TurretEventKind::Sound,
            ..Default::default()
        }
        .put(&mut row, &mut Vec::new());
        let mut notices = Notifications {
            context: &context,
            journal: &mut journal,
            capture: None,
            tick: 1.0,
            store_source: 0,
            presses: &[],
        };
        assert!(notices.handle(gen::EVENT_F3_TURRET, &row).unwrap());
        assert_eq!(journal.seq, 0);
    }
}
