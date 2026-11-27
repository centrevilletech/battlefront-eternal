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
            let mut swbf2_effects = std::mem::take(&mut self.swbf2_effects);
            let drain_res = context
                .world(|w| crate::units::f3::drain_swbf2_effects_into(w, &mut swbf2_effects))
                .map_err(|e| e.message().to_string());
            let emit_res = drain_res.and_then(|()| {
                if let Some(capture) = self.capture.as_ref() {
                    for eff in &swbf2_effects {
                        let mut fields = [("", super::capture::Token::Null); 6];
                        let mut len = 0usize;
                        fields[len] = ("name", super::capture::Token::Interned(eff.name));
                        len += 1;
                        fields[len] = ("position", super::capture::Token::Vector(Some(eff.position)));
                        len += 1;
                        if let Some(forward) = eff.forward {
                            fields[len] = ("forward", super::capture::Token::Vector(Some(forward)));
                            len += 1;
                        }
                        if let Some(duration) = eff.duration {
                            fields[len] = ("duration", super::capture::Token::Number(duration));
                            len += 1;
                        }
                        if let Some((oid, ogen)) = eff.owner {
                            fields[len] = ("ownerId", super::capture::Token::Number(f64::from(oid)));
                            len += 1;
                            fields[len] = ("ownerGeneration", super::capture::Token::Number(ogen));
                            len += 1;
                        }
                        capture.emit(context, tick - 1.0, "effect", 0.0, &fields[..len])?;
                    }
                }
                Ok(())
            });
            swbf2_effects.clear();
            self.swbf2_effects = swbf2_effects;
            emit_res?;
            let mut powers = std::mem::take(&mut self.swbf2_powers);
            let drain_res = context
                .world(|w| crate::combat::powers::drain_published_in(w, &mut powers))
                .map_err(|e| e.message().to_string());
            let emit_res = drain_res.and_then(|()| {
                if let Some(capture) = self.capture.as_ref() {
                    for ev in &powers {
                        emit_power(capture, context, tick - 1.0, ev)?;
                    }
                }
                Ok(())
            });
            powers.clear();
            self.swbf2_powers = powers;
            emit_res?;
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

    /// `F3Items.flush`: a unit whose life or player changed
    /// gets a dispenser binding for every weapon level that dispenses an
    /// item, keyed by `crate::items::owned_key_text` (the character playing
    /// it, or the body's own life when nobody plays it), the same text the
    /// admission bound; the native dispenser later asks for that key. One
    /// list per character and dispenser class, as the 2004 game keeps them:
    /// MaxItems counts a character's items across his lives, and they
    /// outlive him. The first call only learns what the admission bound; a
    /// bot taking a stock body keeps its life but changes its player, and is
    /// bound again then.
    pub(super) fn bind_dispensers(&mut self, context: &Context) -> Result<(), String> {
        self.units.refresh(context, false)?;
        let mut changed = std::mem::take(&mut self.changed_lives);
        changed.clear();
        for (i, unit) in self.units.rows().iter().enumerate() {
            if seen_again(&mut self.lives, (unit.slot, unit.gen, unit.life, unit.character), self.lives_known) {
                changed.push(i);
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

    /// Direct mission deployment can replace a stocked life during admission.
    /// Observe its equipped dispensers before a player can fire on the first tick.
    pub(crate) fn bind_deployed_dispensers(&mut self, context: &Context) -> Result<(), String> {
        self.lives_known = true;
        self.bind_dispensers(context)
    }

    /// The dispenser bindings of the units `bind_dispensers` found with a new life or player.
    fn bind_changed(&self, context: &Context, changed: &[usize]) -> Result<(), String> {
        for &i in changed {
            let unit = &self.units.rows()[i];
            let (slot, generation, life, character) = (unit.slot, unit.gen, unit.life, unit.character);
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
                    let text = crate::items::owned_key_text(&String::from_utf16_lossy(&class), character, slot, generation, life);
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

/// Whether a unit's (slot, generation, life, character) differs from what
/// the host last saw of its slot (and records it): a body bound again. A
/// slot first seen at the start was bound by the admission; one first seen
/// later is new.
fn seen_again(lives: &mut Vec<(u32, u32, f64, Option<crate::store::Ref<gen::Character>>)>,
    now: (u32, u32, f64, Option<crate::store::Ref<gen::Character>>), known: bool) -> bool {
    match lives.iter_mut().find(|entry| entry.0 == now.0) {
        Some(entry) => {
            let changed = entry.1 != now.1 || entry.2 != now.2 || entry.3 != now.3;
            *entry = now;
            changed
        }
        None => {
            lives.push(now);
            known
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::items::owned_key_text;
    use crate::store::Ref;

    #[test]
    fn a_characters_dispenser_list_outlives_his_body() {
        let character = Some(Ref::new(3, 1));
        // Two lives in two bodies: one list, so MaxItems counts across them.
        assert_eq!(owned_key_text("mines", character, 7, 2, 4.0), owned_key_text("mines", character, 11, 1, 5.0));
        // Another character, or another dispenser class, keeps its own.
        assert_ne!(owned_key_text("mines", character, 7, 2, 4.0), owned_key_text("mines", Some(Ref::new(4, 1)), 7, 2, 4.0));
        assert_ne!(owned_key_text("mines", character, 7, 2, 4.0), owned_key_text("bombs", character, 7, 2, 4.0));
        // A body nobody plays keeps one list a life.
        assert_ne!(owned_key_text("mines", None, 7, 2, 4.0), owned_key_text("mines", None, 7, 2, 5.0));
    }

    #[test]
    fn a_body_is_bound_again_when_its_life_or_its_player_changes() {
        let mut lives = Vec::new();
        let (stock, bot) = (Some(Ref::new(5, 1)), Some(Ref::new(9, 3)));
        // The admission bound what it built: the first look learns it.
        assert!(!super::seen_again(&mut lives, (4, 1, 1.0, None), false));
        assert!(!super::seen_again(&mut lives, (4, 1, 1.0, None), true));
        // A bot takes the stock body without a new life: bound again.
        assert!(super::seen_again(&mut lives, (4, 1, 1.0, stock), true));
        assert!(!super::seen_again(&mut lives, (4, 1, 1.0, stock), true));
        // A new life, or another character in it.
        assert!(super::seen_again(&mut lives, (4, 1, 2.0, stock), true));
        assert!(super::seen_again(&mut lives, (4, 1, 2.0, bot), true));
        // A slot first seen after the start is new.
        assert!(super::seen_again(&mut lives, (6, 1, 1.0, None), true));
    }
}

/// One published power event: what happened (`action`), the weapon's class hash,
/// the caster and the target as their life keys, and where the caster stood; a
/// burn also names its effect and the seconds it has left.
fn emit_power(
    capture: &super::capture::Capture,
    context: &crate::abi::context::Context,
    tick: f64,
    ev: &crate::combat::powers::PublishedPower,
) -> Result<(), String> {
    use crate::combat::powers::PowerEventKind as K;
    use super::capture::Token;
    let action = match ev.kind {
        _ if ev.burn.is_some() => "burn",
        K::Cast => "cast",
        K::Hit => "hit",
        K::Thrown => "thrown",
        K::Blocked => "blocked",
        K::Held | K::Buffed => return Ok(()),
    };
    let mut fields = [("", Token::Null); 10];
    let mut len = 0usize;
    let mut put = |key, value| {
        fields[len] = (key, value);
        len += 1;
    };
    put("action", Token::Text(action));
    put("weaponId", Token::Number(f64::from(ev.weapon)));
    put("ownerId", Token::Number(f64::from(ev.caster.slot)));
    put("ownerGeneration", Token::Number(ev.caster.life));
    if let Some(target) = ev.target {
        put("targetId", Token::Number(f64::from(target.slot)));
        put("targetGeneration", Token::Number(target.life));
    }
    if let Some(position) = ev.position {
        put("position", Token::Vector(Some(position)));
    }
    if let Some((effect, seconds)) = ev.burn {
        put("name", Token::Interned(effect));
        put("duration", Token::Number(seconds));
    }
    capture.emit(context, tick, "power", 0.0, &fields[..len])?;
    Ok(())
}
