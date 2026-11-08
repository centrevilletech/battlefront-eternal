//! What the page's `step()` (src/game.js) and the world operation
//! (src/sim/wasm/world.js) wrote into the owners' input banks before
//! `world_step_run`, in their order: the fleet's input header
//! (`F2Sim.foreignIn(true)`), both kernels' body flags and the separation
//! scope (`preparePhase`), the live and tail inputs (`F4Live.prepareStep`,
//! `F4Tail.prepareStep`), and the AI's header with the journal it hears
//! (`ai.prepare`, after `simTime = tick / 60`). The native tick reads the
//! banks; nothing in it writes them.
use super::journal::{Heard, Hearing};
use super::{block_row, put, unit_generation, unit_rows, write_bank, Authority};
use crate::abi::context::Context;
use crate::gen::{
    self, AiPhaseFacts, AiPhaseHeader, AiPhaseReticle, F2Input, F2PadTeam, F2PhaseFlag,
    F3Separation, F4LiveInput, F4TailInput, FfInputs, Vehicles,
};
use crate::world::admit::{Call, Returned};

impl Authority {
    /// The pre-step writes. `now` is the step's `performance.now()` (the
    /// tick input's `at`), in milliseconds.
    pub(super) fn before_step(
        &mut self,
        context: &Context,
        tick: f64,
        dt: f64,
        now: f64,
    ) -> Result<(), String> {
        // F2Sim.foreignIn(true): the clock is performance.now() in seconds,
        // the level the water's (0 without water), the flags poses, flight
        // and cosmetic; the pad rows name no post yet (the slot descriptors'
        // posts are not in the store), and no member, flight or firing row.
        let slots = {
            let words = context
                .export(u32::from(gen::KIND_VEHICLES), 1)
                .map_err(|e| e.message().to_string())?;
            let (row, heap) = block_row(&words);
            let mut fleet = Vehicles::default();
            fleet.take_full(row, heap);
            fleet
        };
        let pads = slots.slots.len();
        let at = (F2Input::WORDS + pads * F2PadTeam::WORDS) as u32;
        let header = F2Input {
            clock: now / 1000.0,
            level: if self.water_level.is_finite() {
                self.water_level
            } else {
                0.0
            },
            flags: (1 << F2PhaseFlag::Poses as u32)
                | (1 << F2PhaseFlag::Flight as u32)
                | (1 << F2PhaseFlag::Cosmetic as u32),
            pads: F2Input::WORDS as u32,
            occupant: slots.spawn_occupant,
            member_length: 0,
            members: at,
            member_count: 0,
            teams: at,
            team_count: 0,
            relations: at,
            flights: at,
            flight_count: 0,
            firing: at,
            end: at + FfInputs::WORDS as u32,
        };
        let mut f2 = put::<{ F2Input::WORDS }>(|row, heap| header.put(row, heap));
        for _ in 0..pads {
            let pad = F2PadTeam {
                has_team: false,
                team: 0.0,
            };
            f2.extend(put::<{ F2PadTeam::WORDS }>(|row, heap| pad.put(row, heap)));
        }
        f2.push(0.0);
        write_bank(context, "F2_INPUT", &f2)?;
        // preparePhase -> bodiesIn: every body's enabled flag, for both
        // kernels. The native owners toggle their own bodies in the movement
        // kernel's flags (hull births and deaths, the animated props); the
        // page saw those as its bodies' `enabled` too. Adopt what changed
        // since the last write, then the buildings' shown states the page
        // toggled (F3Props.show), and hand both kernels the same flags.
        if let Ok(Returned::Bytes(movement)) =
            context.invoke_export("bank:MOVEMENT_FLAGS", &Call::default())
        {
            for (i, (now, was)) in movement.iter().zip(&self.written).enumerate() {
                if now != was && i < self.enabled.len() {
                    self.enabled[i] = *now;
                }
            }
        }
        for (kernel, enabled) in self.toggles.drain(..) {
            if kernel < self.enabled.len() {
                self.enabled[kernel] = enabled;
            }
        }
        let flags = Call {
            scratch: self.enabled.clone(),
            ..Default::default()
        };
        for bank_name in ["bank:COLLISION_ENABLED", "bank:MOVEMENT_FLAGS"] {
            if let Err(e) = context.invoke_export(bank_name, &flags) {
                return Err(format!("{bank_name}: {}", e.message()));
            }
        }
        self.written.clone_from(&self.enabled);
        // preparePhase: a server world separates every pair everywhere.
        let separation = F3Separation {
            global: true,
            camera: [0.0; 3],
        };
        write_bank(
            context,
            "F3_SEPARATION",
            &put::<{ F3Separation::WORDS }>(|row, heap| separation.put(row, heap)),
        )?;
        let live = F4LiveInput { dt, tick };
        write_bank(
            context,
            "F4_LIVE_INPUT",
            &put::<{ F4LiveInput::WORDS }>(|row, heap| live.put(row, heap)),
        )?;
        // The local server's own command carries no human choices.
        let tail = F4TailInput {
            dt,
            tick,
            server: true,
            human_commands: 0,
            human_count: 0,
        };
        write_bank(
            context,
            "F4_TAIL_INPUT",
            &put::<{ F4TailInput::WORDS }>(|row, heap| tail.put(row, heap)),
        )?;
        // ai-world-load.js prepare(dt, tick, true): network views (the
        // players' phase writes the cameras natively), no reticle, the AI's
        // time is simTime, the journal is the sim events since its cursor:
        // each shot or damage record as an AiJournalEvent row, a unit
        // reference being its store reference and the life the record named.
        let heard = {
            let units = unit_rows(context)?;
            let mut rows = Vec::new();
            let records = self.journal.capture(&mut self.journal_cursor);
            for record in records {
                let reference = |heard: Option<Heard>| match heard {
                    Some(h) => gen::AiJournalRef {
                        member: Some(gen::Member::Unit(crate::store::Ref::new(
                            h.slot,
                            unit_generation(&units, h.slot),
                        ))),
                        generation: Some(h.generation),
                        direct: false,
                    },
                    None => gen::AiJournalRef {
                        member: None,
                        generation: None,
                        direct: false,
                    },
                };
                let row = match &record.hearing {
                    Hearing::Shot { source } => gen::AiJournalEvent {
                        sequence: record.seq as f64,
                        kind: gen::AiJournalKind::Shot,
                        source: reference(*source),
                        target: reference(None),
                        attacker: reference(None),
                        attacker_character: None,
                        amount: None,
                        generic: None,
                        typed: None,
                        known: false,
                        point: None,
                    },
                    Hearing::Damage {
                        target,
                        attacker,
                        amount,
                        generic,
                        typed,
                        known,
                        point,
                    } => gen::AiJournalEvent {
                        sequence: record.seq as f64,
                        kind: gen::AiJournalKind::Damage,
                        source: reference(None),
                        target: reference(*target),
                        attacker: reference(*attacker),
                        attacker_character: None,
                        amount: *amount,
                        generic: *generic,
                        typed: *typed,
                        known: *known,
                        point: *point,
                    },
                    Hearing::None => continue,
                };
                rows.push(row);
            }
            rows
        };
        let mut journal_words = Vec::with_capacity(heard.len() * gen::AiJournalEvent::WORDS);
        for row in &heard {
            journal_words.extend(put::<{ gen::AiJournalEvent::WORDS }>(|r, h| row.put(r, h)));
        }
        if !journal_words.is_empty() {
            write_bank(context, "AI_JOURNAL", &journal_words)?;
        }
        let header = AiPhaseHeader {
            facts: AiPhaseFacts {
                dt,
                tick: tick as u32,
                network_views: true,
                lod_disabled: false,
                reticle: AiPhaseReticle {
                    pilot: None,
                    pilot_life: 0.0,
                    target: None,
                    target_life: 0.0,
                },
            },
            time: tick / 60.0,
            paused: false,
            network_secondary: false,
            unit_phase_follows: true,
            cameras: 0,
            events: heard.len() as u32,
            sequence: self.journal.seq as f64,
        };
        write_bank(
            context,
            "AI_HEADER",
            &put::<{ AiPhaseHeader::WORDS }>(|row, heap| header.put(row, heap)),
        )
    }
}
