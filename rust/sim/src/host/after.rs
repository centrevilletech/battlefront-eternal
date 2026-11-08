//! What the page did after the world's step: `worldTick.finish()` drained
//! the unit phase's notices into the game's handlers (drain.rs) and
//! acknowledged the publication it read, `F3Items.flush` bound the new
//! lives' dispensers, `ordnance.sim.refresh` took the rounds' retired lists;
//! then `createMultiplayerWorld` mirrored the players' presses
//! (`mirrorShots`) and their changed views (`mirror()`).
use super::drain::Drain;
use super::{block_row, intern, lease_rows, put, unit_rows, write_bank, Authority};
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
        // (reading takes them) set the page's enabled flags.
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
            let units = unit_rows(context)?;
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
            // The page drained these while its clock still read the tick
            // before (worldTick.complete() follows the step): the capture's
            // records carry that tick.
            let mut drained = Drain {
                tick: tick - 1.0,
                capture: self.capture.as_ref(),
                journal: &mut self.journal,
                presses: &self.presses,
                units: &units,
                leases: &leases,
                toggles: &mut self.toggles,
                prop_bodies: &prop_bodies,
                projectiles: &mut self.projectiles,
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
        for kind in [
            gen::KIND_BOLT,
            gen::KIND_BEAM,
            gen::KIND_GRENADE,
            gen::KIND_ROUND,
            gen::KIND_ORDNANCE,
        ] {
            context
                .export(u32::from(kind), 0)
                .map_err(|e| e.message().to_string())?;
        }
        Ok(())
    }

    /// `F3Items.flush` (src/sim/wasm/f3-items.js): a unit whose life changed
    /// gets a dispenser binding for every weapon level that dispenses an
    /// item, keyed `NAME:body:SLOT:GENERATION:LIFE` where NAME is the
    /// level's class; the native dispenser later asks for that key. The
    /// first call only learns the lives the admission bound.
    pub(super) fn bind_dispensers(&mut self, context: &Context) -> Result<(), String> {
        let units = context
            .export(u32::from(gen::KIND_UNIT), 1)
            .map_err(|e| e.message().to_string())?;
        let stride = units[B::STRIDE] as usize;
        let rows = units[B::ROWS] as usize;
        let heap_at = units[B::HEAP] as usize;
        let heap = &units[heap_at..heap_at + units[B::HEAP_WORDS] as usize];
        let mut changed = Vec::new();
        for i in 0..rows {
            let row = &units[B::WORDS + i * stride..B::WORDS + (i + 1) * stride];
            let (slot, generation) = (row[0] as u32, row[1] as u32);
            let mut unit = gen::Unit::default();
            unit.take_full(row, heap);
            match self.lives.iter_mut().find(|(s, _, _)| *s == slot) {
                Some(entry) => {
                    if entry.1 != generation || entry.2 != unit.generation {
                        *entry = (slot, generation, unit.generation);
                        changed.push((slot, generation, unit));
                    }
                }
                None => {
                    // The first sight of a slot: the admission bound the
                    // lives it built, a later slot is new.
                    self.lives.push((slot, generation, unit.generation));
                    if self.lives_known {
                        changed.push((slot, generation, unit));
                    }
                }
            }
        }
        self.lives_known = true;
        for (slot, generation, unit) in changed {
            let life = unit.generation;
            for weapon in &unit.combat.weapons {
                let levels = context
                    .weapon_levels(weapon.def.0)
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
                        weapon: weapon.def,
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

    /// `mirrorShots()` (src/runtime/multiplayer-world.js): each client's
    /// presses this tick (`players_shots`), by button.
    pub(super) fn mirror_shots(&mut self, context: &Context) -> Result<(), String> {
        for client in &mut self.clients {
            client.presses = [None, None];
        }
        match context.invoke_export("players_shots", &Call::default()) {
            Ok(Returned::Row(words)) if words.len() >= B::WORDS => {
                let (row, heap) = block_row(&words);
                let list = gen::PlayerShotList::take(row, heap);
                for shots in &list.players {
                    if let Some(client) = self.clients.iter_mut().find(|c| c.player == shots.handle)
                    {
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
