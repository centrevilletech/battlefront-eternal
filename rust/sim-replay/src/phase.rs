//! The phases of a world tick, in the order they run: the units of the
//! simulation's capture, replay and port, named once here and in
//! tools/sim-capture/phases.mjs (the same names, the same order; a test on
//! each side holds them together). A capture names a phase by its
//! JavaScript name; every package's replay names it by `Phase`.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Phase {
    /// The snapshot host takes the clients' edges in (host.advanceEdges).
    Edges,
    /// The hit history keeps the last tick's bodies (hitHistory.capture).
    HitHistory,
    /// Each player's commands merged from his mailbox: spawns, team, intents, look, boarding.
    Players,
    /// The players' views the AI's level of detail reads (updateViews).
    Views,
    /// The game step's start: the command, the dormant local player, water, the time, the view camera.
    Begin,
    /// The native world tick and its completed publication.
    WorldTick,
    /// Hulls back to their tick poses (vehicles.restoreTickPose).
    RestorePose,
    /// The AI (ai.update).
    Ai,
    /// Every unit's tick in factory order, with its water and the death regions.
    UnitTick,
    /// The hulls, two 1/120 s substeps on the ground (vehicles.update).
    Vehicles,
    /// Turrets and their gunners (turretSeats.update).
    TurretSeats,
    /// The local player's turret (gunner.update; never on a server world).
    Gunner,
    /// Recon droids (remotes.update, networkRemotes.finish).
    Remotes,
    /// Soldiers pushed apart (separateSoldiers).
    Separate,
    /// Live props: animated props, turrets' idle sweep, traps, stations (live.update).
    Live,
    /// Rounds in flight (ordnance.update).
    Ordnance,
    /// Items: pickups, mines, dropped weapons (items.update).
    Items,
    /// Out of the battlefield too long (battlefield.update).
    Battlefield,
    /// Posts, battle rules, tickets and spawning (updateBattle).
    Battle,
    /// The local player's target reticle (updateTargetHud).
    TargetHud,
    /// Live grenades warn the soldiers near them (ai.grenadeUpdate).
    AiGrenades,
    /// The spawn screen hears the tick (humanSpawn.flush).
    SpawnFlush,
    /// The snapshot host settles the edges the tick used (host.consumedEdges).
    ConsumedEdges,
    /// Snapshots captured, encoded and sent (host.publish).
    Publish,
}

/// Every phase with its JavaScript name, in tick order.
pub const PHASES: [(Phase, &str); 24] = [
    (Phase::Edges, "edges"),
    (Phase::HitHistory, "hitHistory"),
    (Phase::Players, "players"),
    (Phase::Views, "views"),
    (Phase::Begin, "begin"),
    (Phase::WorldTick, "worldTick"),
    (Phase::RestorePose, "restorePose"),
    (Phase::Ai, "ai"),
    (Phase::UnitTick, "unitTick"),
    (Phase::Vehicles, "vehicles"),
    (Phase::TurretSeats, "turretSeats"),
    (Phase::Gunner, "gunner"),
    (Phase::Remotes, "remotes"),
    (Phase::Separate, "separate"),
    (Phase::Live, "live"),
    (Phase::Ordnance, "ordnance"),
    (Phase::Items, "items"),
    (Phase::Battlefield, "battlefield"),
    (Phase::Battle, "battle"),
    (Phase::TargetHud, "targetHud"),
    (Phase::AiGrenades, "aiGrenades"),
    (Phase::SpawnFlush, "spawnFlush"),
    (Phase::ConsumedEdges, "consumedEdges"),
    (Phase::Publish, "publish"),
];

impl Phase {
    /// Its JavaScript name, as captures write it.
    pub fn name(self) -> &'static str {
        PHASES.iter().find(|(p, _)| *p == self).map(|(_, n)| *n).unwrap()
    }
    pub fn from_name(name: &str) -> Option<Phase> {
        PHASES.iter().find(|(_, n)| *n == name).map(|(p, _)| *p)
    }
    pub fn all() -> impl Iterator<Item = Phase> {
        PHASES.iter().map(|(p, _)| *p)
    }
}

impl std::fmt::Display for Phase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}
