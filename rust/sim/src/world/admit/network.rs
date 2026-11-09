//! Dedicated connections use multiplayer-world.js addPlayer and SnapshotHost.addClient.
//! The content and construction receipts survive admission so later clients can
//! prepare their own class stock without reconstructing a second definition space.
use super::{rows::*, session, *};
use crate::{
    abi::context::{Context, Error},
    gen::*,
    world::owner::World,
};
use bfe_content::{content::Content, defs::Defs, files::Files};

pub(crate) struct Network {
    content: Content,
    defs: Defs,
    options: Options,
    ids: Ids,
    definitions: Definitions,
    holders: Vec<bodies::Holder>,
    bindings: session::PlayerBindings,
    handles: Handles,
    recon_keys: Vec<String>,
}

impl Context {
    /// Set the dedicated world's publication epoch before any peers attach.
    /// Admission's tick-zero probe uses epoch one; later matches must publish
    /// the same epoch their welcome and input receivers advertise.
    pub fn world_network_epoch(&self, host: u32, epoch: u32) -> Result<(), Error> {
        self.owned(|w| {
            let host = w
                .wire
                .as_deref_mut()
                .and_then(|wire| wire.hosts.get_mut(host as usize))
                .ok_or_else(|| Error::host("unknown publication host"))?;
            if epoch == 0 || !host.clients.is_empty() {
                return Err(Error::host("publication epoch requires an empty host"));
            }
            host.epoch = f64::from(epoch);
            Ok(())
        })?
    }

    /// Same construction as world_admit, retaining the cold factory catalog.
    pub fn world_network_admit(
        &self,
        files: &dyn Files,
        options: &Options,
    ) -> Result<Admitted, Error> {
        self.owned(|w| -> Result<Admitted, AdmitError> {
            let loader = bfe_content::battle::Loader::new(files);
            let battle = bfe_content::battle::Battle::load(&loader, &options.map, &options.query())
                .map_err(failure)?;
            let mut content = Content::build(battle).map_err(failure)?;
            let defs = Defs::build(&content).map_err(failure)?;
            let mut cx = Cx::new(&content, &defs, files, options, w.sim.strings.clone());
            cx.connection_capacity = 256;
            // Dedicated world-worker.js supplies armySize 32; LocalServer's
            // null choice retains the smaller authored offline populations.
            cx.army_size = Some(crate::battle::roster::MAX_TEAM_SIZE);
            let mut rows = Rows {
                setup: rows::setup(&content.battle),
                ..Default::default()
            };
            let mut admitted = Admitted {
                setup: rows.setup.clone(),
                ..Default::default()
            };
            sequence::run_stages(
                w,
                &content,
                &mut cx,
                &mut rows,
                &mut admitted,
                &sequence::ORDER,
            )?;
            // Both admission entries hand the host the same gameplay, grenade
            // spin, drop and seat streams before any dedicated tick can draw.
            sequence::push_page_streams(w, &mut cx)?;
            admitted.units = spawn::units(w)?;
            let state = (cx.ids, cx.definitions, cx.holders, cx.player);
            // The local admission exercises the shared construction path, but its
            // Character is released before a network client can claim a slot.
            let setup = w
                .input
                .players
                .pool
                .get(admitted.handles.player as usize)
                .ok_or_else(|| failure("missing local player".into()))?;
            let character = setup.character.clone();
            let recon_key = setup.recon_key;
            retire_recon(w, character.as_ref(), recon_key);
            crate::players::module::players_drop_in(w, admitted.handles.player);
            if let Some(character) = character {
                release(w, character);
            }
            crate::wire::host::wire_host_op_in(w, admitted.handles.host as usize, 1, 1, 0., 0.);
            // Only UnitFactory's infantry catalog survives construction. The
            // owning collision, fleet and AI services already installed these
            // world assets; later addPlayer prepares actors, clips and weapons.
            // Retaining both copies charges every connection for map geometry
            // that its class stock can never read.
            content.weapon_packs = Vec::new();
            content.vehicle_models = None;
            content.vehicle_extras = None;
            content.props_models = None;
            content.vehicle_templates = Vec::new();
            content.vehicle_blockers = None;
            content.prop_bodies = Vec::new();
            content.prop_clips = Vec::new();
            content.death_regions = Vec::new();
            content.route_words = None;
            content.placed_items = Vec::new();
            // Keep the scalar kill plane, but release the collision loader's
            // authored triangle arrays after the kernel owns its native banks.
            let collision = &mut content.collision;
            collision.positions = std::rc::Rc::from([]);
            collision.indices = std::rc::Rc::from([]);
            collision.polygons = std::rc::Rc::from([]);
            collision.terrain_positions = std::rc::Rc::from([]);
            collision.terrain_indices = std::rc::Rc::from([]);
            collision.tri_bounds = Vec::new();
            collision.grid = None;
            w.network = Some(Box::new(Network {
                content,
                defs,
                options: options.clone(),
                ids: state.0,
                definitions: state.1,
                holders: state.2,
                bindings: state.3,
                handles: admitted.handles,
                recon_keys: Vec::new(),
            }));
            Ok(admitted)
        })?
        .map_err(|e| Error::host(e.to_string()))
    }

    /// addPlayer reserves a named Character and stages one body per playable class.
    pub fn world_network_join(
        &self,
        files: &dyn Files,
        peer: u32,
        id: &str,
        name: &str,
        spectator: bool,
        auto: bool,
        team_select: bool,
        mailbox: u32,
        at: f64,
    ) -> Result<session::LocalPlayer, Error> {
        self.owned(|w| {
            let mut network = w
                .network
                .take()
                .ok_or_else(|| failure("network admission missing".into()))?;
            let result = network.join(
                w,
                files,
                peer,
                id,
                name,
                spectator,
                auto,
                team_select,
                mailbox,
                at,
            );
            w.network = Some(network);
            result
        })?
        .map_err(|e| Error::host(e.to_string()))
    }

    /// SnapshotHost links carry transport measurements and native lane counters.
    pub fn world_network_discarded(&self, player: u32) -> Result<f64, Error> {
        self.owned(|w| {
            w.input
                .players
                .pool
                .get(player as usize)
                .map_or(0., |p| p.actions.discarded)
        })
    }

    /// removePlayer releases the roster Character after the host removes its edges.
    pub fn world_network_leave(&self, peer: u32, player: u32) -> Result<(), Error> {
        self.owned(|w| {
            let character = w
                .input
                .players
                .pool
                .get(player as usize)
                .and_then(|p| p.character.clone());
            let recon_key = w
                .input
                .players
                .pool
                .get(player as usize)
                .and_then(|p| p.recon_key);
            retire_recon(w, character.as_ref(), recon_key);
            crate::players::module::players_stop_in(w, player);
            crate::players::module::players_drop_in(w, player);
            if let Some(character) = character {
                release(w, character);
            }
            if let Some(n) = &w.network {
                crate::wire::store_source::network_peer(w, n.handles.store_source, peer, None);
            }
        })
    }
}

fn failure(message: String) -> AdmitError {
    AdmitError::Content {
        stage: Stage::Player,
        message,
    }
}
fn record<T>(
    w: &mut World,
    words: usize,
    value: &T,
    put: impl FnOnce(&T, &mut [f64], &mut Vec<f64>),
) {
    w.sim.calls.result(words, |r, h| put(value, r, h));
}
// RemoteDroids.retire leaves detached droids and strikes with their original
// context while ending control for the departing connection.
fn retire_recon(w: &mut World, character: Option<&F4Handle>, key: Option<u32>) {
    let Some((h, key)) = character.zip(key) else {
        return;
    };
    let Some(unit) = crate::battle::f4_connections::wire_view_in(&w.battle, &w.sim.store, h)
        .and_then(|v| v.unit)
    else {
        return;
    };
    let Some(unit_row) = w.sim.store.unit.get(unit) else {
        return;
    };
    let input = F3ReconBinding {
        action: F3ReconAction::Retire,
        key,
        owner: F3UnitKey::of(unit, unit_row),
        ..Default::default()
    };
    record(w, F3ReconBinding::WORDS, &input, F3ReconBinding::put);
    crate::recon::module::f3_recon_command_in(w);
}
fn release(w: &mut World, character: F4Handle) {
    let cmd = F4RosterCommand {
        op: RoOperation::Release,
        target: Some(character),
        ..Default::default()
    };
    record(w, F4RosterCommand::WORDS, &cmd, F4RosterCommand::put);
    crate::battle::f4_roster::f4_roster_command_in(w);
}
fn result<T>(words: &[f64], take: impl FnOnce(&[f64], &[f64]) -> T) -> T {
    let heap = words[BlockHeader::HEAP] as usize;
    take(&words[BlockHeader::WORDS..], &words[heap..])
}
impl Network {
    fn join(
        &mut self,
        w: &mut World,
        files: &dyn Files,
        peer: u32,
        id: &str,
        name: &str,
        spectator: bool,
        auto: bool,
        team_select: bool,
        mailbox: u32,
        at: f64,
    ) -> Result<session::LocalPlayer, AdmitError> {
        if !(1..=256).contains(&peer) || w.input.mailboxes.get(mailbox as usize).is_none() {
            return Err(failure("invalid network connection".into()));
        }
        // addPlayer uses roster.counts(), whose team lists include the
        // reserved Characters even when the public battle list is sparse.
        let command = F4RosterCommand {
            op: RoOperation::Counts,
            ..Default::default()
        };
        record(w, F4RosterCommand::WORDS, &command, F4RosterCommand::put);
        let words = crate::battle::f4_roster::f4_roster_command_in(w)
            .ok_or_else(|| failure("roster unavailable".into()))?;
        let counts = result(words, F4RosterResult::take).counts;
        let team = counts
            .iter()
            .min_by(|a, b| a.humans.total_cmp(&b.humans))
            .map_or(1., |c| c.team);
        let key = bfe_content::js::number_to_string(team);
        let roster = |team: &str| {
            bfe_content::mission::playable_classes(
                self.content
                    .battle
                    .world
                    .get("mission")
                    .and_then(|m| m.get("teams"))
                    .and_then(|t| t.get(team)),
            )
        };
        let class_name = roster(&key)
            .first()
            .cloned()
            .ok_or_else(|| failure("no playable class".into()))?;
        let classes = [roster("1"), roster("2")].concat();
        let mut cx = Cx::new(
            &self.content,
            &self.defs,
            files,
            &self.options,
            w.sim.strings.clone(),
        );
        cx.player = self.bindings.clone();
        cx.ids = std::mem::take(&mut self.ids);
        cx.definitions = std::mem::take(&mut self.definitions);
        cx.holders = std::mem::take(&mut self.holders);
        // UnitFactory's numeric registry bound includes each connection's
        // class stock. Grow it during admission before staging reaches it;
        // no later class switch or respawn may allocate a new body bank.
        let needed = (w.sim.store.units.next_id as u32).saturating_add(classes.len() as u32 + 1);
        if cx
            .ids
            .battle
            .as_ref()
            .and_then(|b| b.construction_limit)
            .unwrap_or(0)
            < needed
        {
            let limit = needed.saturating_add(512);
            w.reserve_spawn_bodies(limit);
            if let Some(facts) = &mut cx.ids.battle {
                facts.construction_limit = Some(limit);
            }
        }
        let prepared = (|| {
            for class in classes {
                session::prepare_class(&mut cx, &class)?;
                spawn::prepare(w, &mut cx, &class)?;
            }
            Ok::<_, AdmitError>(())
        })();
        self.ids = cx.ids;
        self.definitions = cx.definitions;
        self.holders = cx.holders;
        prepared?;
        let character = if spectator {
            None
        } else {
            let cmd = F4RosterCommand {
                op: RoOperation::Reserve,
                team,
                player_id: PlayerId::Str(w.sim.strings.intern_str(id)),
                name: w.sim.strings.intern_str(name),
                ..Default::default()
            };
            record(w, F4RosterCommand::WORDS, &cmd, F4RosterCommand::put);
            let words = crate::battle::f4_roster::f4_roster_command_in(w)
                .ok_or_else(|| failure("roster unavailable".into()))?;
            Some(
                result(words, F4RosterResult::take)
                    .handle
                    .ok_or_else(|| failure("team full".into()))?,
            )
        };
        // F3Recon.admitKey is stable by authenticated identity, not by a
        // transport slot that a different client can reuse after leave.
        let recon_key = if spectator {
            None
        } else {
            let index = self
                .recon_keys
                .iter()
                .position(|key| key == id)
                .unwrap_or_else(|| {
                    self.recon_keys.push(id.to_string());
                    self.recon_keys.len() - 1
                });
            Some(index as u32)
        };
        let mut unit_id = 1;
        let mut generation = 0.;
        if let Some(h) = &character {
            let r = crate::battle::f4_core::resolve_character_ref(&w.battle, &w.sim.store, h)
                .ok_or_else(|| failure("invalid Character".into()))?;
            let binding = F3PlayerBinding {
                character: r,
                local: false,
                combat_random: self.bindings.combat_random,
                weapon_random: self.bindings.weapon_random,
                recon_key,
                recon_seed: rows::recon::seed(self.options.seed, id),
                body: None,
            };
            w.bind_spawn_player(&binding);
            for op in [F4ConnectionOp::Admit, F4ConnectionOp::Spawn] {
                let cmd = F4ConnectionCommand {
                    op,
                    target: h.clone(),
                    class_name: Some(w.sim.strings.intern_str(&class_name)),
                    ordinal: peer as f64,
                    ..Default::default()
                };
                record(
                    w,
                    F4ConnectionCommand::WORDS,
                    &cmd,
                    F4ConnectionCommand::put,
                );
                let words = crate::battle::f4_connections::f4_connection_command_in(w)
                    .ok_or_else(|| failure("connection unavailable".into()))?;
                let view = result(words, F4ConnectionView::take);
                if !view.accepted {
                    release(w, h.clone());
                    return Err(failure("no eligible spawn".into()));
                }
                if let Some(r) = view.unit {
                    unit_id = r.slot;
                    generation = w.sim.store.unit.get(r).map_or(0., |u| u.generation);
                }
            }
            if !auto {
                let cmd = F4ConnectionCommand {
                    op: F4ConnectionOp::Unspawn,
                    target: h.clone(),
                    ..Default::default()
                };
                record(
                    w,
                    F4ConnectionCommand::WORDS,
                    &cmd,
                    F4ConnectionCommand::put,
                );
                crate::battle::f4_connections::f4_connection_command_in(w);
            }
        }
        let setup = PlayerSetup {
            mailbox,
            character: character.clone(),
            spectator,
            reliable_fire: true,
            separate_actions: true,
            team_select,
            recon_key,
            ordinal: peer as f64,
            team,
            at,
            ..Default::default()
        };
        record(w, PlayerSetup::WORDS, &setup, PlayerSetup::put);
        let player = crate::players::module::players_create_in(w);
        if player == 0 {
            if let Some(h) = character {
                release(w, h);
            }
            return Err(failure("no player capacity".into()));
        }
        crate::players::module::players_outcomes_enable_in(w, player);
        crate::wire::store_source::network_peer(
            w,
            self.handles.store_source,
            peer,
            Some(WireStorePeer {
                id: peer,
                character: character.clone(),
                mailbox,
            }),
        );
        Ok(session::LocalPlayer {
            mailbox,
            player,
            character,
            unit_id,
            generation,
            team,
            class_name,
            team_select,
            tick: 0.,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bfe_content::files::DirFiles;

    #[test]
    fn network_admission_binds_names_and_distinct_source_characters() {
        let Ok(root) = std::env::var("BFE_CONTENT_DIR") else {
            return;
        };
        let files = DirFiles::new(root);
        let context = Context::new();
        let options = Options {
            map: "nab2".into(),
            side: "rep".into(),
            enhanced: false,
            seed: 1,
            heroes: false,
            unit: None,
        };
        context.world_network_admit(&files, &options).unwrap();
        for (peer, name) in [(1, "First"), (2, "Second")] {
            let mailbox = context
                .owned(|w| {
                    let codes = session::input_codes()
                        .iter()
                        .map(|code| w.sim.strings.intern_str(code))
                        .collect();
                    let setup = InputMailboxSetup {
                        epoch: 1.,
                        capacity: 256.,
                        codes,
                    };
                    record(w, InputMailboxSetup::WORDS, &setup, InputMailboxSetup::put);
                    crate::input::module::input_mailbox_create_in(w)
                })
                .unwrap();
            let player = context
                .world_network_join(&files, peer, name, name, false, true, false, mailbox, 1000.)
                .unwrap();
            context
                .owned(|w| {
                    let h = player.character.as_ref().unwrap();
                    let r =
                        crate::battle::f4_core::resolve_character_ref(&w.battle, &w.sim.store, h)
                            .unwrap();
                    let c = w.sim.store.character.get(r).unwrap();
                    assert_eq!(
                        String::from_utf16_lossy(w.sim.strings.units(c.name.unwrap())),
                        name
                    );
                    assert_eq!(c.team, peer as f64);
                    assert!(c.human);
                    let body = w.sim.store.unit.get(c.unit.unwrap()).unwrap();
                    assert_eq!(body.team, c.team);
                    assert!(body.human);
                    assert_eq!(body.network_human, Some(true));
                })
                .unwrap();
        }
        context.dispose().unwrap();
    }
}
