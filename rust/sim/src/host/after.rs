//! What the client did after the world's step: `worldTick.finish()` drained
//! the unit phase's notices into the game's handlers (drain.rs) and
//! acknowledged the publication it read, `F3Items.flush` bound the new
//! lives' dispensers, `ordnance.sim.refresh` took the rounds' retired lists;
//! then `createMultiplayerWorld` mirrored the players' presses
//! (`mirrorShots`) and their changed views (`mirror()`).
use super::drain::Drain;
use super::{block_row, intern, lease_rows, put, write_bank, Authority};
use crate::abi::context::Context;
use crate::abi::context_step::bank;
use crate::gen::{self, BlockHeader as B, F3Acknowledgement, F3Boundary, PlayerViews};
use crate::world::admit::{Call, Returned};

impl Authority {
    /// The unit phase's journal after the step: every event drained
    /// (`drainEvents`, `consumeEvent`) and the publication acknowledged
    /// (`sync`), so the owner commits its baselines; and the rounds' kinds
    /// re-read in the mirror's mode, which takes their retired lists.
    /// `players_events` is the word offset where the players' events end.
    pub(super) fn acknowledge(
        &mut self,
        context: &Context,
        tick: f64,
        players_events: usize,
    ) -> Result<(), String> {
        // F2Sim.bodiesOut: the hulls' toggled bodies since the last read
        // (reading takes them) set the client's enabled flags.
        let mut changes = Vec::new();
        context
            .read_bank(bank::F2_DELTA, &mut changes)
            .map_err(|e| e.message().to_string())?;
        if changes.len() >= gen::F2BodyHeader::WORDS {
            let header = gen::F2BodyHeader::take(&changes, &[]);
            let mut at = gen::F2BodyHeader::WORDS;
            for _ in 0..header.toggled {
                if at + gen::F2BodyToggle::WORDS > changes.len() {
                    break;
                }
                let toggle = gen::F2BodyToggle::take(&changes[at..], &[]);
                if (toggle.kernel as usize) < self.enabled.len() {
                    self.enabled[toggle.kernel as usize] = u8::from(toggle.enabled);
                }
                at += gen::F2BodyToggle::WORDS;
            }
        }
        let mut descriptor = Vec::new();
        context
            .read_bank(bank::F3_DESCRIPTOR, &mut descriptor)
            .map_err(|e| e.message().to_string())?;
        // drainEvents: a new journal epoch starts the read over; the
        // notices before the players' events carry the previous tick's
        // clock (`unitsSim.eventClock`), the rest this tick's.
        if descriptor.len() >= F3Boundary::WORDS {
            let epoch = descriptor[F3Boundary::EVENT_EPOCH] as u32;
            if epoch != self.event_epoch {
                self.event_epoch = epoch;
                self.event_offset = 0;
            }
            let mut events = Vec::new();
            context
                .read_bank(bank::EVENTS, &mut events)
                .map_err(|e| e.message().to_string())?;
            let (before, after) = ((tick - 1.0) / 60.0, tick / 60.0);
            self.units.refresh(context, true)?;
            let leases = lease_rows(context)?;
            let prop_bodies = context
                .prop_body_kernels()
                .map_err(|e| e.message().to_string())?;
            // `u.networkShots` as the last mirror left them, by the body
            // each client's player had then.
            self.presses.clear();
            self.presses.extend(
                self.clients
                    .iter()
                    .filter_map(|c| c.unit.map(|unit| (unit, c.presses))),
            );
            // The client drained these while its clock still read the tick
            // before (worldTick.complete() follows the step): the capture's
            // records carry that tick.
            let mut drained = Drain {
                recon_definitions: &self.recon_definitions, // matrix-b
                tick: tick - 1.0,
                capture: self.capture.as_ref(),
                content: &self.content,
                journal: &mut self.journal,
                presses: &self.presses,
                units: &self.units,
                leases: &leases,
                toggles: &mut self.toggles,
                prop_bodies: &prop_bodies,
                projectiles: &mut self.projectiles,
                dynamics: &mut self.dynamics,
            };
            self.event_offset = drained.run(context, &events, self.event_offset, |at| {
                if at < players_events {
                    before
                } else {
                    after
                }
            })?;
            self.journal.time = after;
        }
        self.bind_dispensers(context)?;
        if descriptor.len() >= F3Boundary::WORDS {
            let ack = F3Acknowledgement {
                version: descriptor[F3Boundary::VERSION] as u32,
                event_epoch: descriptor[F3Boundary::EVENT_EPOCH] as u32,
                event_offset: descriptor[F3Boundary::EVENT_WORDS] as u32,
            };
            write_bank(
                context,
                "F3_ACKNOWLEDGEMENT",
                &put::<{ F3Acknowledgement::WORDS }>(|row, heap| ack.put(row, heap)),
            )?;
        }
        // The client re-read the rounds' kinds in the mirror's mode, which
        // took their retired lists; nothing else of that read was used.
        context
            .with_store(|s| {
                s.bolt.retired.clear();
                s.beam.retired.clear();
                s.grenade.retired.clear();
                s.round.retired.clear();
            })
            .map_err(|e| e.message().to_string())
    }

    /// `F3Items.flush`: a unit whose life changed
    /// gets a dispenser binding for every weapon level that dispenses an
    /// item, keyed `NAME:body:SLOT:GENERATION:LIFE` where NAME is the
    /// level's class; the native dispenser later asks for that key. The
    /// first call only learns the lives the admission bound.
    pub(super) fn bind_dispensers(&mut self, context: &Context) -> Result<(), String> {
        self.units.refresh(context, false)?;
        let mut changed = std::mem::take(&mut self.changed_lives);
        changed.clear();
        for (i, unit) in self.units.rows().iter().enumerate() {
            let (slot, generation) = (unit.slot, unit.gen);
            match self.lives.iter_mut().find(|(s, _, _)| *s == slot) {
                Some(entry) => {
                    if entry.1 != generation || entry.2 != unit.life {
                        *entry = (slot, generation, unit.life);
                        changed.push(i);
                    }
                }
                None => {
                    // The first sight of a slot: the admission bound the
                    // lives it built, a later slot is new.
                    self.lives.push((slot, generation, unit.life));
                    if self.lives_known {
                        changed.push(i);
                    }
                }
            }
        }
        self.lives_known = true;
        // A new life binds its weapons: read them (the same rows, in order).
        if !changed.is_empty() {
            self.units.refresh(context, true)?;
        }
        let result = self.bind_changed(context, &changed);
        self.changed_lives = changed;
        result
    }

    /// The dispenser bindings of the units `bind_dispensers` found with a new life.
    fn bind_changed(&self, context: &Context, changed: &[usize]) -> Result<(), String> {
        for &i in changed {
            let unit = &self.units.rows()[i];
            let (slot, generation, life) = (unit.slot, unit.gen, unit.life);
            for weapon in self.units.weapons(unit) {
                let levels = context
                    .weapon_levels(weapon.0)
                    .map_err(|e| e.message().to_string())?;
                for (level, facts) in levels.iter().enumerate() {
                    if !facts.item {
                        continue;
                    }
                    let class = facts.class.clone().unwrap_or_default();
                    let name = context
                        .intern(&class)
                        .map_err(|e| e.message().to_string())?;
                    let text = format!(
                        "{}:body:{}:{}:{}",
                        String::from_utf16_lossy(&class),
                        slot,
                        generation,
                        jsrt::numfmt::number_to_string(life)
                    );
                    let owned_key = intern(context, &text)?;
                    let binding = gen::F3DispenserBinding {
                        key: gen::F3UnitKey {
                            slot,
                            generation,
                            life,
                        },
                        weapon: *weapon,
                        level: level as u32,
                        name,
                        owned_key,
                    };
                    let mut row = [0.0; gen::F3DispenserBinding::WORDS];
                    let mut heap = Vec::new();
                    binding.put(&mut row, &mut heap);
                    let call = Call {
                        row: row.to_vec(),
                        heap,
                        ..Default::default()
                    };
                    match context.invoke_export("f3_dispenser_binding", &call) {
                        Ok(Returned::Scalar(v)) if v != 0.0 => {}
                        Ok(_) => return Err("stale dispenser construction".into()),
                        Err(e) => return Err(format!("f3_dispenser_binding: {}", e.message())),
                    }
                }
            }
        }
        Ok(())
    }

    /// `mirrorShots()`: the presses of
    /// each player the players' phase listed this tick (`players_shots`),
    /// by button, onto his body (`u.networkShots`); a player it did not
    /// list, or one without a body, keeps what his body had.
    pub(super) fn mirror_shots(&mut self, context: &Context) -> Result<(), String> {
        match context.invoke_export("players_shots", &Call::default()) {
            Ok(Returned::Row(words)) if words.len() >= B::WORDS => {
                let (row, heap) = block_row(&words);
                let list = gen::PlayerShotList::take(row, heap);
                for shots in &list.players {
                    let client = self.clients.iter_mut().find(|c| c.player == shots.handle);
                    if let Some(client) = client.filter(|c| c.unit.is_some()) {
                        client.presses = [shots.fire0, shots.fire1];
                    }
                }
                Ok(())
            }
            Ok(_) => Ok(()),
            Err(e) => Err(format!("players_shots: {}", e.message())),
        }
    }

    /// `mirror()`: the players this tick changed (`native.changed()`, which
    /// takes them); a retired player's client leaves, another's body
    /// follows his view (`p.unit = view.unit ?? p.unit`). The views stay
    /// readable until the next tick for the caller's own mirrors.
    pub(super) fn mirror(&mut self, context: &Context) -> Result<(), String> {
        let views = context
            .players_views(|words| {
                words.map(|w| {
                    let (row, heap) = block_row(w);
                    PlayerViews::take(row, heap).players
                })
            })
            .map_err(|e| e.message().to_string())?;
        self.views = views.unwrap_or_default();
        for view in &self.views {
            if view.deleted {
                self.clients.retain(|c| c.player != view.handle);
            } else if let Some(unit) = view.unit {
                if let Some(client) = self.clients.iter_mut().find(|c| c.player == view.handle) {
                    client.unit = Some(unit.slot);
                }
            }
        }
        Ok(())
    }
}
