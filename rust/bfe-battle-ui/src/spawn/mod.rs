//! Classic spawn screen module (`src/spawn-screen/`).

pub mod controller;
pub mod input;
pub mod layout;
pub mod model;
pub mod policy;
pub mod view;

pub use controller::SpawnScreenController;
pub use input::{SpawnHitTarget, SpawnInput, SpawnInputAction, TAB_REPEAT};
pub use layout::{
    classic_layout, spawn_layout_for_family, ClassSlotLayout, ClassicSpawnLayout, COMMIT_WAIT,
    SLOTS,
};
pub use model::{
    choose_frontline_post, human_entry, queue_human_command, CameraShot, HumanCommand,
    HumanSpawnFacts, OpenReason, PostChangeReason, SpawnClassEntry, SpawnEvent, SpawnModel,
    SpawnPort, SpawnPostEntry, SpawnRequest, SpawnState, SpawnTeamFacts, TeamSwitchSource,
    HUMAN_COMMAND_LIMIT, NAME_LIMIT,
};
pub use policy::{
    playable_classes, side_of_class, spawn_policy, spawn_policy_from_query, team_of_class,
    AutoSpawnPick, MissionSpec, MissionTeamSpec, SpawnParams, SpawnPolicy, DEFAULT_UNIT,
};
pub use view::{
    fallback_logo_for_side, spawn_map_place, ClassicSpawnView, MapProjection, Spawn3dPlacement,
    SpawnLogoPlacement, SpawnMapPlacement, SpawnSound,
};
