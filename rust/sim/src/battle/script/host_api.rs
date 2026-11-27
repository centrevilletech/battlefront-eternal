//! Bound Battlefront II (2005) mission script host functions backed by the
//! authoritative battle [`RuleActions`] interface.

use crate::battle::rules::events::{
    ObjectHandle, ObjectProperty, Outcome, Reinforcements, RuleError,
};
use crate::battle::spawning::TEAM_COUNT;

use super::adapter::{
    arg_flag, ascii_bytes_eq_ignore_case, check_bytes, check_int, check_string, check_string_id,
    f64_to_lua, lua_to_exact_handle, lua_to_finite_f64, lua_to_i32, opt_int, opt_string,
    push_str, set_unknown_global_handler, table_ints, table_strings,
    with_actions, ScriptCallCtx, ScriptError, ScriptNativeFn, ScriptValue, ScriptVm,
    MAX_EXACT_LUA_HANDLE,
};
use super::state::{
    AiGoalRecord, AiGoalTarget, AudioStreamRecord, BattleOutcome, CriticalSystemRecord,
    DataFileRequest, DiagnosticKind, HostState, ListenerFamily, ObjectiveEntry,
    ScriptNotification, ScriptPhase, UnitClassSetup, MAX_AI_GOALS, MAX_AUDIO_STREAMS,
    MAX_CAMERA_SHOTS, MAX_CRITICAL_SYSTEMS, MAX_OBJECTIVES,
    TRANSIENT_ATTRIBUTED_KILLER_HANDLE,
};

/// Register all supported 2005 mission host functions and platform globals on `vm`.
pub(crate) fn register_all(vm: &mut ScriptVm<HostState>) -> Result<(), ScriptError> {
    // Platform identifier global expected by 2005 PC mission scripts.
    // Undefined globals remain nil under normal Lua global lookup rules.
    let pc_str = vm.intern_str("PC")?;
    vm.set_global("gPlatformStr", ScriptValue::String(pc_str))?;

    let bindings: &[(&str, ScriptNativeFn<HostState>)] = &[
        // Group 1: Content loading & memory setup
        ("ScriptCB_DoFile", host_dofile),
        ("ReadDataFile", host_read_data_file),
        ("StealArtistHeap", host_noop),
        ("SetPS2ModelMemory", host_noop),
        ("SetMemoryPoolSize", host_noop),
        ("ClearWalkers", host_unsupported),
        ("AddWalkerType", host_unsupported),
        ("SetWorldExtents", host_set_world_extents),
        ("SetMaxFlyHeight", host_set_max_fly_height),
        ("SetMaxPlayerFlyHeight", host_set_max_player_fly_height),
        ("SetMinFlyHeight", host_set_min_fly_height),
        ("SetMinPlayerFlyHeight", host_set_min_player_fly_height),
        ("SetDenseEnvironment", host_set_dense_environment),
        ("SetParticleLODBias", host_noop),
        ("SetAIVehicleNotifyRadius", host_set_ai_vehicle_notify_radius),
        ("AddCameraShot", host_add_camera_shot),
        ("ScriptCB_GetPlatform", host_get_platform),
        ("ScriptCB_IsMissionSetupSaved", host_is_mission_setup_saved),
        ("ScriptCB_LoadMissionSetup", host_load_mission_setup),
        ("ScriptCB_DisableFlyerShadows", host_noop),
        // Group 2: Teams & unit classes
        ("SetTeamName", host_set_team_name),
        ("SetTeamIcon", host_set_team_icon),
        ("SetUnitCount", host_set_unit_count),
        ("SetReinforcementCount", host_set_reinforcement_count),
        ("AddUnitClass", host_add_unit_class),
        ("AddHeroClass", host_add_hero_class),
        ("SetSpawnDelay", host_set_spawn_delay),
        ("EnableSPHeroRules", host_enable_sp_hero_rules),
        ("ScriptCB_GetASSNumBots", host_get_ass_num_bots),
        ("ScriptCB_SetNumBots", host_set_num_bots),
        ("GetCharacterTeam", host_get_character_team),
        ("GetReinforcementCount", host_get_reinforcement_count),
        ("AddReinforcements", host_add_reinforcements),
        ("IsCharacterHuman", host_is_character_human),
        // Group 3: AI goals
        ("AddAIGoal", host_add_ai_goal),
        ("ClearAIGoals", host_clear_ai_goals),
        ("DeleteAIGoal", host_delete_ai_goal),
        // Group 4: World objects & properties
        ("SetProperty", host_set_property),
        ("KillObject", host_kill_object),
        ("IsObjectAlive", host_is_object_alive),
        ("GetEntityName", host_get_entity_name),
        ("GetObjectTeam", host_get_object_team),
        ("SetObjectTeam", host_set_object_team),
        ("GetObjectShield", host_get_object_shield),
        ("EnableBuildingLockOn", host_enable_building_lock_on),
        ("SetAIDamageThreshold", host_set_ai_damage_threshold),
        ("DisableBarriers", host_disable_barriers),
        ("BlockPlanningGraphArcs", host_block_planning_graph_arcs),
        ("AddDeathRegion", host_add_death_region),
        // Group 5: Events & callbacks (family-checked release forms)
        ("OnCharacterDeath", host_on_character_death),
        ("ReleaseCharacterDeath", host_release_character_death),
        ("OnTicketCountChange", host_on_ticket_count_change),
        ("ReleaseTicketCountChange", host_release_ticket_count_change),
        ("OnTimerElapse", host_on_timer_elapse),
        ("ReleaseTimerElapse", host_release_timer_elapse),
        ("OnTeamPointsChange", host_on_team_points_change),
        ("ReleaseTeamPointsChange", host_release_team_points_change),
        ("OnObjectKillName", host_on_object_kill_name),
        ("ReleaseObjectKill", host_release_object_kill),
        ("OnObjectDamageName", host_on_object_damage_name),
        ("ReleaseObjectDamage", host_release_object_damage),
        ("OnObjectRespawnName", host_on_object_respawn_name),
        ("ReleaseObjectRespawn", host_release_object_respawn),
        // Group 6: Timers
        ("CreateTimer", host_create_timer),
        ("FindTimer", host_find_timer),
        ("SetTimerValue", host_set_timer_value),
        ("GetTimerValue", host_get_timer_value),
        ("SetTimerRate", host_set_timer_rate),
        ("StartTimer", host_start_timer),
        ("StopTimer", host_stop_timer),
        ("DestroyTimer", host_destroy_timer),
        ("ShowTimer", host_show_timer),
        // Group 7: Objectives & scoring
        ("AddMissionObjective", host_add_mission_objective),
        ("ActivateObjective", host_activate_objective),
        ("CompleteObjective", host_complete_objective),
        ("SetFlagGameplayType", host_set_flag_gameplay_type),
        ("SetTeamPoints", host_set_team_points),
        ("AddTeamPoints", host_add_team_points),
        ("GetTeamPoints", host_get_team_points),
        ("MissionVictory", host_mission_victory),
        ("MissionDefeat", host_mission_defeat),
        ("ScriptCB_GetAssaultScoreLimit", host_get_assault_score_limit),
        ("ScriptCB_GetHuntScoreLimit", host_get_hunt_score_limit),
        ("ScriptCB_GetHuntMaxTimeLimit", host_get_hunt_max_time_limit),
        ("ScriptCB_GetUberScoreLimit", host_get_uber_score_limit),
        ("ScriptCB_SetUberScoreLimit", host_set_uber_score_limit),
        ("ScriptCB_ShowHuntScoreLimit", host_show_hunt_score_limit),
        ("IsCampaign", host_is_campaign),
        ("GetWorldFilename", host_get_world_filename),
        // Group 8: Messages & HUD
        ("ShowTeamPoints", host_show_team_points),
        ("ShowMessageText", host_show_message_text),
        // Deprecated no-op in the 2005 API.
        ("ShowObjectiveTextPopup", host_unsupported),
        ("AddMissionHint", host_add_mission_hint),
        ("MapHideCommandPosts", host_map_hide_command_posts),
        ("DisableSmallMapMiniMap", host_disable_small_map_minimap),
        ("ScriptCB_GetNumCameras", host_get_num_cameras),
        // Group 9: Sound & voice-over
        ("OpenAudioStream", host_open_audio_stream),
        ("AudioStreamAppendSegments", host_audio_stream_append_segments),
        ("BroadcastVoiceOver", host_broadcast_voice_over),
        ("SetBleedingVoiceOver", host_set_bleeding_vo),
        ("SetLowReinforcementsVoiceOver", host_set_low_reinforcements_vo),
        ("SetOutOfBoundsVoiceOver", host_set_out_of_bounds_vo),
        ("SetAmbientMusic", host_set_ambient_music),
        ("SetVictoryMusic", host_set_victory_music),
        ("SetDefeatMusic", host_set_defeat_music),
        ("SetSoundEffect", host_set_sound_effect),
        ("SetAttackingTeam", host_set_attacking_team),
        ("ScriptCB_EnableHeroMusic", host_enable_hero_music),
        ("ScriptCB_EnableHeroVO", host_enable_hero_vo),
        ("ScriptCB_SetDopplerFactor", host_set_doppler_factor),
        ("ScaleSoundParameter", host_scale_sound_parameter),
        ("AudioStreamComplete", host_audio_stream_complete),
        // Group 10: Space Assault
        ("AddLandingRegion", host_add_landing_region),
        ("SpaceAssaultEnable", host_space_assault_enable),
        ("SpaceAssaultSetupTeamNumbers", host_space_teams),
        ("SpaceAssaultSetupBitmaps", host_space_assault_setup_bitmaps),
        ("SpaceAssaultAddCriticalSystem", host_space_assault_add_critical_system),
        ("SpaceAssaultLinkCriticalSystems", host_space_assault_link_critical_systems),
        ("SpaceAssaultGetScoreLimit", host_space_assault_get_score_limit),
        ("AddSpaceAssaultDestroyPoints", host_add_space_assault_destroy_points),
    ];

    for &(name, func) in bindings {
        vm.register_fn(name, func)?;
    }

    set_unknown_global_handler(vm, |ctx, name| {
        charge(ctx)?;
        // Count unknown calls in BOTH strict and permissive modes before returning/throwing.
        if name.len() > 256 { return Err(ScriptError::ResourceLimit("mission global name")); }
        ctx.host_mut().charge(name.len().div_ceil(32) as u32)?;
        ctx.host_mut().diagnostics.record_unbound(name);
        if ctx.host().config.strict_unbound {
            Err(ScriptError::runtime(format!(
                "unbound host function: {name}"
            )))
        } else {
            Ok(())
        }
    });

    Ok(())
}

fn charge(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    let n = ctx.arg_count();
    if n > 64 { return Err(ScriptError::ResourceLimit("mission arguments")); }
    let mut cost = 1 + n / 8;
    for i in 0..n {
        if let ScriptValue::String(id) = ctx.arg(i) {
            let len = ctx.vm().string_bytes(id).ok_or(ScriptError::InvalidHandle("argument string"))?.len();
            if len > 4096 { return Err(ScriptError::ResourceLimit("mission string operand")); }
            cost += len.div_ceil(32);
        }
    }
    let units = u32::try_from(cost).unwrap_or(u32::MAX);
    ctx.charge_work(units)?;
    ctx.host_mut().charge(units)
}

fn setup_only(ctx: &ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    if ctx.host().phase == ScriptPhase::Tick { Err(ScriptError::ResourceLimit("setup declaration in running mission")) } else { Ok(()) }
}

fn label(ctx: &mut ScriptCallCtx<'_, HostState>, i: usize) -> Result<std::rc::Rc<str>, ScriptError> {
    let id = check_string_id(ctx, i)?;
    let (vm, host) = ctx.split();
    let value = ScriptValue::String(id);
    for (root, name) in &host.labels {
        if matches!(vm.get_ref(*root), Some(ScriptValue::String(key)) if key == id) {
            return Ok(name.clone());
        }
    }
    let name: std::rc::Rc<str> = super::adapter::identifier(vm, id)?.into();
    if host.labels.len() == super::state::MAX_NOTIFICATIONS {
        if let Some((old, _)) = host.labels.pop_front() {
            vm.release_ref(old);
        }
    }
    let root = vm.create_ref(value)?;
    host.labels.push_back((root, name.clone()));
    Ok(name)
}

pub(super) fn record_rule_error(host: &mut HostState, err: RuleError, op: &str) {
    match err {
        RuleError::UnknownName => {
            host.diagnostics.missing_objects = host.diagnostics.missing_objects.saturating_add(1);
        }
        RuleError::AmbiguousName => {
            host.diagnostics.ambiguous_objects =
                host.diagnostics.ambiguous_objects.saturating_add(1);
        }
        RuleError::NotAvailable => {
            host.diagnostics.not_available_ops =
                host.diagnostics.not_available_ops.saturating_add(1);
        }
        RuleError::StaleHandle | RuleError::InvalidValue | RuleError::LimitReached => {
            host.diagnostics.refused_writes = host.diagnostics.refused_writes.saturating_add(1);
            host.diagnostics.record_error(
                host.tick,
                host.phase,
                None,
                DiagnosticKind::RefusedWrite,
                format!("{op}: {err:?}"),
            );
        }
    }
}

fn host_unsupported(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    record_rule_error(ctx.host_mut(),RuleError::NotAvailable,"unavailable mission operation");
    ctx.push_return(ScriptValue::Bool(false))
}

fn host_noop(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let hints = &mut ctx.host_mut().diagnostics.ignored_platform_hints;
    *hints = hints.saturating_add(1);
    push_str(ctx, "ignored")
}

// ---------------------------------------------------------------------------
// Group 1: Content loading & memory setup
// ---------------------------------------------------------------------------

fn host_dofile(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    setup_only(ctx)?;
    let sid = check_string_id(ctx, 0)?;
    let (canonical, chunk_idx) = {
        let req = ctx.vm().string_bytes(sid).ok_or(ScriptError::InvalidHandle("string"))?;
        let mut found = None;
        for (i, c) in ctx.host().chunks.iter().enumerate() {
            if ascii_bytes_eq_ignore_case(c.name.as_bytes(), req) {
                found = Some((c.name.clone(), i));
                break;
            }
        }
        let Some((name, idx)) = found else {
            let disp = super::adapter::identifier(ctx.vm(), sid)?;
            return Err(ScriptError::runtime(format!(
                "ScriptCB_DoFile: script '{disp}' not found"
            )));
        };
        (name, idx)
    };
    let work = ctx.host().chunks[chunk_idx].bytes.len().div_ceil(32) + chunk_idx + 1;
    ctx.charge_work(u32::try_from(work).unwrap_or(u32::MAX))?;
    if ctx.host().setup.loaded_scripts.len() >= 256 { return Err(ScriptError::ResourceLimit("mission chunk trace")); }
    ctx.host_mut().setup.loaded_scripts.push(canonical);
    let (vm, host) = ctx.split();
    let cl = host.load_chunk(vm, chunk_idx)?;
    let mut out = Vec::new();
    ctx.call_into(ScriptValue::Closure(cl), &[], &mut out)?;
    for val in out {
        ctx.push_return(val)?;
    }
    Ok(())
}

fn host_read_data_file(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    setup_only(ctx)?;
    let path = check_string(ctx, 0)?;
    let mut args = Vec::with_capacity(ctx.arg_count().saturating_sub(1));
    for i in 1..ctx.arg_count() {
        if let Some(s) = opt_string(ctx, i)? {
            args.push(s);
        }
    }
    ctx.host_mut()
        .setup
        .data_files
        .push(DataFileRequest { path, args });
    Ok(())
}

fn host_set_world_extents(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let n = ctx.check_number(0)?;
    if let Some(v) = lua_to_finite_f64(n) {
        ctx.host_mut().setup.world_extents = Some(v);
    }
    Ok(())
}

fn host_set_max_fly_height(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let n = ctx.check_number(0)?;
    if let Some(v) = lua_to_finite_f64(n) {
        ctx.host_mut().setup.max_fly_height = Some(v);
    }
    Ok(())
}

fn host_set_max_player_fly_height(
    ctx: &mut ScriptCallCtx<'_, HostState>,
) -> Result<(), ScriptError> {
    charge(ctx)?;
    let n = ctx.check_number(0)?;
    if let Some(v) = lua_to_finite_f64(n) {
        ctx.host_mut().setup.max_player_fly_height = Some(v);
    }
    Ok(())
}

fn host_set_min_fly_height(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let n = ctx.check_number(0)?;
    if let Some(v) = lua_to_finite_f64(n) {
        ctx.host_mut().setup.min_fly_height = Some(v);
    }
    Ok(())
}

fn host_set_min_player_fly_height(
    ctx: &mut ScriptCallCtx<'_, HostState>,
) -> Result<(), ScriptError> {
    charge(ctx)?;
    let n = ctx.check_number(0)?;
    if let Some(v) = lua_to_finite_f64(n) {
        ctx.host_mut().setup.min_player_fly_height = Some(v);
    }
    Ok(())
}

fn host_set_dense_environment(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let flag = arg_flag(ctx, 0, false);
    ctx.host_mut().setup.dense_environment = flag;
    Ok(())
}

fn host_set_ai_vehicle_notify_radius(
    ctx: &mut ScriptCallCtx<'_, HostState>,
) -> Result<(), ScriptError> {
    charge(ctx)?;
    let n = ctx.check_number(0)?;
    if let Some(v) = lua_to_finite_f64(n) {
        ctx.host_mut().setup.ai_vehicle_notify_radius = Some(v);
    }
    Ok(())
}

fn host_add_camera_shot(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    setup_only(ctx)?;
    let argc = ctx.arg_count();
    if argc != 7 {
        let host = ctx.host_mut();
        host.diagnostics.record_error(
            host.tick,
            host.phase,
            None,
            DiagnosticKind::InvalidArgument,
            format!("AddCameraShot requires 7 numeric arguments (got {argc})"),
        );
        return Err(ScriptError::runtime(
            "AddCameraShot requires 7 arguments (qw, qx, qy, qz, px, py, pz)",
        ));
    }
    let mut shot = [0.0f64; 7];
    for (i, slot) in shot.iter_mut().enumerate() {
        let n = ctx.check_number(i)?;
        let Some(v) = lua_to_finite_f64(n) else {
            return Err(ScriptError::runtime("AddCameraShot argument must be finite"));
        };
        *slot = v;
    }
    if ctx.host().setup.camera_shots.len() < MAX_CAMERA_SHOTS {
        ctx.host_mut().setup.camera_shots.push(shot);
    } else {
        record_rule_error(ctx.host_mut(), RuleError::LimitReached, "AddCameraShot");
    }
    Ok(())
}

fn host_get_platform(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    push_str(ctx, "PC")
}

fn host_is_mission_setup_saved(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    ctx.push_return(ScriptValue::Bool(false))
}

fn host_load_mission_setup(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let tbl = ctx.vm_mut().create_table()?;
    ctx.push_return(ScriptValue::Table(tbl))
}

// ---------------------------------------------------------------------------
// Group 2: Teams & unit classes
// ---------------------------------------------------------------------------

/// Only playable teams have admission rosters, points and reinforcements.
fn playable_team(ctx: &mut ScriptCallCtx<'_, HostState>, operation: &str) -> Result<Option<i32>, ScriptError> {
    let number = ctx.check_number(0)?;
    if number.fract() == 0.0 && (1.0..TEAM_COUNT as f32).contains(&number) {
        return Ok(Some(number as i32));
    }
    record_rule_error(ctx.host_mut(), RuleError::InvalidValue, operation);
    ctx.push_return(ScriptValue::Bool(false))?;
    Ok(None)
}

fn host_set_team_name(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    setup_only(ctx)?;
    let Some(team) = playable_team(ctx, "SetTeamName")? else { return Ok(()) };
    let name = check_string(ctx, 1)?;
    ctx.host_mut().team_setup_mut(team).name = Some(name);
    Ok(())
}

fn host_set_team_icon(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    setup_only(ctx)?;
    let Some(team) = playable_team(ctx, "SetTeamIcon")? else { return Ok(()) };
    let mut icons = Vec::with_capacity(ctx.arg_count().saturating_sub(1));
    for i in 1..ctx.arg_count() {
        if let Some(icon) = opt_string(ctx, i)? {
            icons.push(icon);
        }
    }
    ctx.host_mut().team_setup_mut(team).icons = icons;
    Ok(())
}

fn host_set_unit_count(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    setup_only(ctx)?;
    let Some(team) = playable_team(ctx, "SetUnitCount")? else { return Ok(()) };
    let count = check_int(ctx, 1)?.max(0) as u32;
    ctx.host_mut().team_setup_mut(team).unit_count = Some(count);
    Ok(())
}

fn host_set_reinforcement_count(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let Some(team) = playable_team(ctx, "SetReinforcementCount")? else { return Ok(()) };
    let raw = check_int(ctx, 1)?;
    let target = if raw < 0 {
        Reinforcements::Unlimited
    } else {
        Reinforcements::Finite(raw as u32)
    };

    if matches!(
        ctx.host().phase,
        ScriptPhase::Chunk | ScriptPhase::PreInit | ScriptPhase::Init
    ) {
        // Cold load/init records the final initial value without emitting events.
        ctx.host_mut().team_setup_mut(team).initial_reinforcements = Some(target);
        return Ok(());
    }

    let result = with_actions(|a| {
        let prev = a.reinforcements(f64::from(team)).ok();
        let res = a.set_reinforcements(f64::from(team), target);
        (prev, res)
    });
    match result {
        Some((prev, Ok(()))) => {
            if let Some(slot) = team_slot(team) {
                ctx.host_mut().cached_reinforcements[slot] = match target {
                    Reinforcements::Unlimited => -1,
                    Reinforcements::Finite(n) => n.min(i32::MAX as u32) as i32,
                };
            }
            if matches!(target, Reinforcements::Finite(_)) && prev != Some(target) {
                ctx.host_mut().emitted_ticket_seq =
                    ctx.host().emitted_ticket_seq.saturating_add(1);
            }
        }
        Some((_, Err(err))) => record_rule_error(ctx.host_mut(), err, "SetReinforcementCount"),
        None => record_rule_error(ctx.host_mut(), RuleError::NotAvailable, "SetReinforcementCount"),
    }
    Ok(())
}

fn host_add_unit_class(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    setup_only(ctx)?;
    let Some(team) = playable_team(ctx, "AddUnitClass")? else { return Ok(()) };
    let class_name = check_string(ctx, 1)?;
    let min_units = opt_int(ctx, 2, 0)?;
    let max_units = if ctx.arg(3).is_nil() {
        None
    } else {
        Some(check_int(ctx, 3)?)
    };
    ctx.host_mut().team_setup_mut(team).classes.push(UnitClassSetup {
        class_name,
        min_units,
        max_units,
    });
    Ok(())
}

fn host_add_hero_class(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    setup_only(ctx)?;
    if let Some(class_name) = opt_string(ctx, 0)? {
        ctx.host_mut().setup.hero_classes.push(class_name);
    }
    Ok(())
}

fn host_set_spawn_delay(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    setup_only(ctx)?;
    let wave = ctx.check_number(0)?;
    let slot = ctx.check_number(1)?;
    if let Some(v) = lua_to_finite_f64(wave) {
        ctx.host_mut().setup.spawn_wave_delay = Some(v);
    }
    if let Some(v) = lua_to_finite_f64(slot) {
        ctx.host_mut().setup.spawn_slot_delay = Some(v);
    }
    Ok(())
}

fn host_enable_sp_hero_rules(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    // In the 2005 game this call itself has no gameplay side effect and returns nothing;
    // record the call only as an observable mode flag.
    ctx.host_mut().match_flags.sp_hero_rules = true;
    Ok(())
}

fn host_get_ass_num_bots(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let bots = ctx.host().config.effective_assault_num_bots() as f32;
    ctx.push_return(ScriptValue::Number(bots))
}

fn host_set_num_bots(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let count = opt_int(ctx, 0, 0)?.max(0) as u32;
    ctx.host_mut().match_flags.num_bots = Some(count);
    Ok(())
}

fn host_get_character_team(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let team = match ctx.arg(0) {
        ScriptValue::Number(n) => {
            if let Some(handle) = lua_to_exact_handle(n) {
                resolve_character_team(ctx.host(), handle)
            } else {
                0
            }
        }
        _ => 0,
    };
    ctx.push_return(ScriptValue::Number(team as f32))
}

pub(crate) fn resolve_character_team(host: &HostState, handle: u32) -> i32 {
    if let Some((_, _, Some(c), Some(id))) = &host.active_object { if *id == handle { return c.team; } }
    // During OnCharacterDeath, honor the authoritative event snapshot for victim
    // and killer (launch-snapshot team attribution outlives team changes).
    if let Some(active) = host.active_death {
        if handle == active.victim_handle {
            return active.victim_team;
        }
        if active.killer_handle == Some(handle) {
            return active.killer_team.unwrap_or(0);
        }
        if handle == TRANSIENT_ATTRIBUTED_KILLER_HANDLE {
            return active.killer_team.unwrap_or(0);
        }
    }
    let Some(entry) = host.character(handle) else {
        return 0;
    };
    let sim_ref = entry.sim_ref;
    match with_actions(|a| a.character_team(sim_ref)) {
        Some(Ok(t)) if t.is_finite() => t as i32,
        _ => entry.last_team,
    }
}

fn host_get_reinforcement_count(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let team = check_int(ctx, 0)?;
    if matches!(
        ctx.host().phase,
        ScriptPhase::Chunk | ScriptPhase::PreInit | ScriptPhase::Init
    ) {
        let count = match ctx
            .host()
            .team_setup(team)
            .and_then(|t| t.initial_reinforcements)
        {
            Some(Reinforcements::Unlimited) => -1,
            Some(Reinforcements::Finite(n)) => n.min(i32::MAX as u32) as i32,
            None => 0,
        };
        return ctx.push_return(ScriptValue::Number(count as f32));
    }

    let count = match with_actions(|a| a.reinforcements(f64::from(team))) {
        Some(Ok(Reinforcements::Unlimited)) => -1,
        Some(Ok(Reinforcements::Finite(n))) => n.min(i32::MAX as u32) as i32,
        Some(Err(err)) => {
            record_rule_error(ctx.host_mut(), err, "GetReinforcementCount");
            0
        }
        None => 0,
    };
    ctx.push_return(ScriptValue::Number(count as f32))
}

fn host_add_reinforcements(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let Some(team) = playable_team(ctx, "AddReinforcements")? else { return Ok(()) };
    let delta = check_int(ctx, 1)?;
    if matches!(
        ctx.host().phase,
        ScriptPhase::Chunk | ScriptPhase::PreInit | ScriptPhase::Init
    ) {
        let setup = ctx.host_mut().team_setup_mut(team);
        if let Some(Reinforcements::Finite(cur)) = setup.initial_reinforcements {
            setup.initial_reinforcements = Some(Reinforcements::Finite(cur.saturating_add_signed(delta)));
        }
        return Ok(());
    }

    let result = with_actions(|a| {
        let prev = a.reinforcements(f64::from(team)).ok();
        let res = a.add_reinforcements(f64::from(team), delta);
        (prev, res)
    });
    match result {
        Some((prev, Ok(next))) => {
            if let Some(slot) = team_slot(team) {
                ctx.host_mut().cached_reinforcements[slot] = match next {
                    Reinforcements::Unlimited => -1,
                    Reinforcements::Finite(n) => n.min(i32::MAX as u32) as i32,
                };
            }
            if matches!(next, Reinforcements::Finite(_)) && prev != Some(next) {
                ctx.host_mut().emitted_ticket_seq =
                    ctx.host().emitted_ticket_seq.saturating_add(1);
            }
        }
        Some((_, Err(err))) => record_rule_error(ctx.host_mut(), err, "AddReinforcements"),
        None => record_rule_error(ctx.host_mut(), RuleError::NotAvailable, "AddReinforcements"),
    }
    Ok(())
}

fn host_is_character_human(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let human = match ctx.arg(0) {
        ScriptValue::Number(n) => {
            if let Some(handle) = lua_to_exact_handle(n) {
                if let Some(active) = ctx.host().active_death {
                    if handle == active.victim_handle {
                        active.victim_human
                    } else if active.killer_handle == Some(handle) {
                        active.killer_human
                    } else if handle == TRANSIENT_ATTRIBUTED_KILLER_HANDLE {
                        false
                    } else {
                        query_character_human(ctx.host(), handle)
                    }
                } else {
                    query_character_human(ctx.host(), handle)
                }
            } else {
                false
            }
        }
        _ => false,
    };
    ctx.push_return(ScriptValue::Bool(human))
}

fn query_character_human(host: &HostState, handle: u32) -> bool {
    if let Some((_, _, Some(c), Some(id))) = &host.active_object { if *id == handle { return c.human; } }
    let Some(entry) = host.character(handle) else {
        return false;
    };
    let sim_ref = entry.sim_ref;
    match with_actions(|a| a.character_is_human(sim_ref)) {
        Some(Ok(h)) => h,
        _ => entry.last_human,
    }
}

// ---------------------------------------------------------------------------
// Group 3: AI goals
// ---------------------------------------------------------------------------

fn parse_ai_goal_target(
    ctx: &mut ScriptCallCtx<'_, HostState>,
    idx: usize,
) -> Result<Option<AiGoalTarget>, ScriptError> {
    match ctx.arg(idx) {
        ScriptValue::Nil => Ok(None),
        ScriptValue::String(sid) => Ok(Some(AiGoalTarget::Name(
            ctx.vm().string_bytes(sid).ok_or(ScriptError::InvalidHandle("string"))?.to_vec(),
        ))),
        ScriptValue::Number(n) => {
            let Some(handle) = lua_to_exact_handle(n) else {
                return Err(ScriptError::runtime(
                    "AddAIGoal target character handle out of exact range",
                ));
            };
            ctx.host().character(handle).map(|c| Some(AiGoalTarget::Character(c.sim_ref))).ok_or(ScriptError::InvalidHandle("goal character"))
        }
        ScriptValue::Userdata(uid) => {
            let (tag, payload) = ctx.vm().userdata_info(uid).ok_or(ScriptError::InvalidHandle("userdata"))?;
            if let Some(object) = ctx.host().decode_object(tag,payload) { return Ok(Some(AiGoalTarget::Object(object))); }
            Err(ScriptError::runtime("AddAIGoal received invalid object userdata"))
        }
        other => Err(ScriptError::runtime(format!(
            "AddAIGoal target argument {} has unsupported type {}",
            idx + 1,
            other.type_name()
        ))),
    }
}

fn host_add_ai_goal(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    use crate::ai::goals::GoalKind;

    charge(ctx)?;
    if !(3..=5).contains(&ctx.arg_count()) {
        return Err(ScriptError::runtime("AddAIGoal takes three to five arguments"));
    }
    let team = ctx.check_number(0)?;
    let name = check_string_id(ctx, 1)?;
    let kind = ctx.vm().string_bytes(name).and_then(GoalKind::parse);
    let weight = ctx.check_number(2)?;
    let target1 = parse_ai_goal_target(ctx, 3)?;
    let target2 = parse_ai_goal_target(ctx, 4)?;
    let kind = kind.filter(|kind| crate::ai::goals::accepts_target(*kind, &AiGoalTarget::view(target1.as_ref())));
    let Some(kind) = kind.filter(|_| valid_goal_team(team) && weight.is_finite() && weight > 0.0) else {
        record_rule_error(ctx.host_mut(), RuleError::InvalidValue, "AddAIGoal");
        return ctx.push_return(ScriptValue::Nil);
    };
    let host = ctx.host_mut();
    let Some(id) = (host.ai_goals.len() < MAX_AI_GOALS).then(|| host.goal_handles.issue()).flatten() else {
        record_rule_error(host, RuleError::LimitReached, "AddAIGoal");
        return ctx.push_return(ScriptValue::Nil);
    };
    let slot = host.ai_goals.iter().enumerate()
        .position(|(i, row)| usize::from(row.slot) != i).unwrap_or(host.ai_goals.len());
    host.ai_goals.insert(slot, AiGoalRecord {
        slot: slot as u8, id, team: team as u8, kind, weight, target1, target2,
    });
    host.goal_revision = host.goal_revision.wrapping_add(1);
    ctx.push_return(ScriptValue::Number(id as f32))
}

fn host_clear_ai_goals(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let team = ctx.check_number(0)?;
    if !valid_goal_team(team) {
        record_rule_error(ctx.host_mut(), RuleError::InvalidValue, "ClearAIGoals");
        return Ok(());
    }
    let host = ctx.host_mut();
    let before = host.ai_goals.len();
    host.ai_goals.retain(|goal| goal.team != team as u8);
    if host.ai_goals.len() != before {
        host.goal_revision = host.goal_revision.wrapping_add(1);
    }
    Ok(())
}

fn valid_goal_team(team: f32) -> bool {
    (0.0..crate::ai::goals::GOAL_TEAMS as f32).contains(&team) && team.fract() == 0.0
}

fn host_delete_ai_goal(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    if ctx.arg(0).is_nil() {
        return Ok(());
    }
    let ScriptValue::Number(n) = ctx.arg(0) else {
        return Ok(());
    };
    let Some(id) = lua_to_exact_handle(n) else {
        return Ok(());
    };
    let host = ctx.host_mut();
    if let Some(index) = host.ai_goals.iter().position(|goal| goal.id == id) {
        host.ai_goals.remove(index);
        host.goal_revision = host.goal_revision.wrapping_add(1);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Group 4: World objects & properties (authoritative via RuleActions)
// ---------------------------------------------------------------------------

/// Resolve a Lua object argument (userdata or raw name string) against the
/// authoritative simulation index. NEVER fabricates missing objects.
fn resolve_authoritative_object(
    ctx: &mut ScriptCallCtx<'_, HostState>,
    idx: usize,
    op: &str,
) -> Result<Option<ObjectHandle>, ScriptError> {
    match ctx.arg(idx) {
        ScriptValue::Nil => Ok(None),
        ScriptValue::Userdata(uid) => {
            let (tag, payload) = ctx.vm().userdata_info(uid).ok_or(ScriptError::InvalidHandle("userdata"))?;
            if let Some(object) = ctx.host().decode_object(tag,payload) { return Ok(Some(object)); }
            record_rule_error(ctx.host_mut(), RuleError::StaleHandle, op);
            Ok(None)
        }
        ScriptValue::String(sid) => {
            // Only valid UTF-8 raw bytes can match an admitted UTF-8 mission name;
            // non-UTF-8 raw bytes are never lossily transcoded to false matches.
            let name_opt = ctx.vm().string_str(sid);
            let res = match name_opt {
                Some(name) => match with_actions(|a| a.find_object(name)) {
                    Some(r) => r,
                    None => Err(RuleError::NotAvailable),
                },
                None => Err(RuleError::UnknownName),
            };
            match res {
                Ok(handle) => Ok(Some(handle)),
                Err(err) => {
                    record_rule_error(ctx.host_mut(), err, op);
                    Ok(None)
                }
            }
        }
        _ => {
            record_rule_error(ctx.host_mut(), RuleError::InvalidValue, op);
            Ok(None)
        }
    }
}

fn host_set_property(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let Some(handle) = resolve_authoritative_object(ctx, 0, "SetProperty")? else {
        return ctx.push_return(ScriptValue::Bool(false));
    };
    let prop_sid = check_string_id(ctx, 1)?;
    let val = ctx.arg(2);
    let num_val = ctx.vm().to_number(val).and_then(lua_to_finite_f64);
    let flag_val = arg_flag(ctx, 2, false);

    // Match property key directly on exact raw ASCII bytes (case-insensitive,
    // whitespace significant; zero allocation).
    let property = {
        let raw = ctx.vm().string_bytes(prop_sid).ok_or(ScriptError::InvalidHandle("string"))?;
        if ascii_bytes_eq_ignore_case(raw, b"curhealth") {
            num_val.map(|v| ObjectProperty::Health(if v > 0.0 { v } else { 0.0 }))
        } else if ascii_bytes_eq_ignore_case(raw, b"maxhealth") {
            num_val.map(|v| ObjectProperty::MaxHealth(if v > 0.0 { v } else { 0.0 }))
        } else if ascii_bytes_eq_ignore_case(raw, b"team") {
            num_val.map(|v| ObjectProperty::Team(f64::from(lua_to_i32(v as f32))))
        } else if ascii_bytes_eq_ignore_case(raw, b"iscollidable") {
            Some(ObjectProperty::Collidable(flag_val))
        } else if ascii_bytes_eq_ignore_case(raw, b"isvisible") {
            Some(ObjectProperty::Visible(flag_val))
        } else if ascii_bytes_eq_ignore_case(raw, b"physicsactive") {
            Some(ObjectProperty::PhysicsActive(flag_val))
        } else if ascii_bytes_eq_ignore_case(raw, b"islocked") {
            Some(ObjectProperty::Locked(flag_val))
        } else if ascii_bytes_eq_ignore_case(raw, b"curshield") {
            num_val.map(|v| ObjectProperty::CurrentShield(ctx.host().shield_write(&handle, v)))
        } else if ascii_bytes_eq_ignore_case(raw, b"maxshield") {
            num_val.map(|v| ObjectProperty::MaxShield(if v > 0.0 { v } else { 0.0 }))
        } else if ascii_bytes_eq_ignore_case(raw, b"addshield") {
            num_val.map(ObjectProperty::AddShield)
        } else {
            None
        }
    };

    let Some(prop) = property else {
        record_rule_error(ctx.host_mut(), RuleError::NotAvailable, "SetProperty");
        return ctx.push_return(ScriptValue::Bool(false));
    };

    match with_actions(|a| a.set_property(&handle, prop)) {
        Some(Ok(())) => {}
        other => {
            record_rule_error(ctx.host_mut(), other.and_then(Result::err).unwrap_or(RuleError::NotAvailable), "SetProperty");
            return ctx.push_return(ScriptValue::Bool(false));
        }
    }
    ctx.push_return(ScriptValue::Bool(true))
}

fn host_kill_object(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let Some(handle) = resolve_authoritative_object(ctx, 0, "KillObject")? else {
        return ctx.push_return(ScriptValue::Bool(false));
    };
    match with_actions(|a| a.kill_object(&handle)) {
        Some(Ok(())) => {}
        other => {
            record_rule_error(ctx.host_mut(), other.and_then(Result::err).unwrap_or(RuleError::NotAvailable), "KillObject");
            return ctx.push_return(ScriptValue::Bool(false));
        }
    }
    ctx.push_return(ScriptValue::Bool(true))
}

fn host_is_object_alive(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let Some(handle) = resolve_authoritative_object(ctx, 0, "IsObjectAlive")? else {
        return ctx.push_return(ScriptValue::Bool(false));
    };
    let alive = match with_actions(|a| a.object_alive(&handle)) {
        Some(Ok(v)) => v,
        Some(Err(err)) => {
            record_rule_error(ctx.host_mut(), err, "IsObjectAlive");
            false
        }
        None => {
            record_rule_error(ctx.host_mut(), RuleError::NotAvailable, "IsObjectAlive");
            false
        }
    };
    ctx.push_return(ScriptValue::Bool(alive))
}

fn host_get_entity_name(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    match ctx.arg(0) {
        // Preserve exact raw Lua string ID without UTF-8/Latin-1 re-encoding.
        ScriptValue::String(sid) => ctx.push_return(ScriptValue::String(sid)),
        _ => {
            let Some(handle) = resolve_authoritative_object(ctx, 0, "GetEntityName")? else {
                return ctx.push_return(ScriptValue::Nil);
            };
            let name_opt = with_actions(|a| a.object_name(&handle).map(|name| ctx.vm_mut().intern_bytes(name.as_bytes())));
            match name_opt {
                Some(Ok(id)) => ctx.push_return(ScriptValue::String(id?)),
                Some(Err(err)) => {
                    record_rule_error(ctx.host_mut(), err, "GetEntityName");
                    ctx.push_return(ScriptValue::Nil)
                }
                None => {
                    record_rule_error(ctx.host_mut(), RuleError::NotAvailable, "GetEntityName");
                    ctx.push_return(ScriptValue::Nil)
                }
            }
        }
    }
}

fn host_get_object_team(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let Some(handle) = resolve_authoritative_object(ctx, 0, "GetObjectTeam")? else {
        return ctx.push_return(ScriptValue::Number(0.0));
    };
    let team = match with_actions(|a| a.object_team(&handle)) {
        Some(Ok(t)) => f64_to_lua(t),
        Some(Err(err)) => {
            record_rule_error(ctx.host_mut(), err, "GetObjectTeam");
            0.0
        }
        None => {
            record_rule_error(ctx.host_mut(), RuleError::NotAvailable, "GetObjectTeam");
            0.0
        }
    };
    ctx.push_return(ScriptValue::Number(team))
}

fn host_set_object_team(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let Some(handle) = resolve_authoritative_object(ctx, 0, "SetObjectTeam")? else {
        return ctx.push_return(ScriptValue::Bool(false));
    };
    let team = check_int(ctx, 1)?;
    match with_actions(|a| a.set_property(&handle, ObjectProperty::Team(f64::from(team)))) {
        Some(Ok(())) => {}
        other => {
            record_rule_error(ctx.host_mut(), other.and_then(Result::err).unwrap_or(RuleError::NotAvailable), "SetObjectTeam");
            return ctx.push_return(ScriptValue::Bool(false));
        }
    }
    ctx.push_return(ScriptValue::Bool(true))
}

fn host_get_object_shield(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let Some(handle) = resolve_authoritative_object(ctx, 0, "GetObjectShield")? else {
        return ctx.push_return(ScriptValue::Nil);
    };
    let shields = with_actions(|a| a.object_shields(&handle)).unwrap_or(Err(RuleError::NotAvailable));
    let (cur, max, add) = match shields {
        Ok(triple) => triple,
        Err(err) => {
            record_rule_error(ctx.host_mut(), err, "GetObjectShield");
            return ctx.push_return(ScriptValue::Nil);
        }
    };
    ctx.push_return(ScriptValue::Number(f64_to_lua(cur)))?;
    ctx.push_return(ScriptValue::Number(f64_to_lua(max)))?;
    ctx.push_return(ScriptValue::Number(f64_to_lua(add)))
}

fn host_enable_building_lock_on(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let Some(handle) = resolve_authoritative_object(ctx, 0, "EnableBuildingLockOn")? else {
        return ctx.push_return(ScriptValue::Bool(false));
    };
    let enabled = arg_flag(ctx, 1, false);
    match with_actions(|a| a.set_property(&handle, ObjectProperty::LockOn(enabled))) {
        Some(Ok(())) => {}
        other => {
            record_rule_error(ctx.host_mut(), other.and_then(Result::err).unwrap_or(RuleError::NotAvailable), "EnableBuildingLockOn");
            return ctx.push_return(ScriptValue::Bool(false));
        }
    }
    ctx.push_return(ScriptValue::Bool(true))
}

fn host_set_ai_damage_threshold(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let Some(handle) = resolve_authoritative_object(ctx, 0, "SetAIDamageThreshold")? else {
        return ctx.push_return(ScriptValue::Bool(false));
    };
    let frac = ctx.check_number(1)?;
    let Some(v) = lua_to_finite_f64(frac) else {
        record_rule_error(ctx.host_mut(), RuleError::InvalidValue, "SetAIDamageThreshold");
        return ctx.push_return(ScriptValue::Bool(false));
    };
    match with_actions(|a| a.set_property(&handle, ObjectProperty::AiDamageThreshold(v))) {
        Some(Ok(())) => {}
        other => {
            record_rule_error(ctx.host_mut(), other.and_then(Result::err).unwrap_or(RuleError::NotAvailable), "SetAIDamageThreshold");
            return ctx.push_return(ScriptValue::Bool(false));
        }
    }
    ctx.push_return(ScriptValue::Bool(true))
}

fn host_disable_barriers(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let name = check_string(ctx, 0)?;
    let result = with_actions(|actions| actions.set_passage_barrier(&name, false));
    if let Some(Ok(())) = result {
        ctx.push_return(ScriptValue::Bool(true))
    } else {
        record_rule_error(ctx.host_mut(), RuleError::NotAvailable, "mission region or navigation");
        ctx.push_return(ScriptValue::Bool(false))
    }
}

fn host_block_planning_graph_arcs(
    ctx: &mut ScriptCallCtx<'_, HostState>,
) -> Result<(), ScriptError> {
    charge(ctx)?;
    check_string_id(ctx, 0)?;
    record_rule_error(ctx.host_mut(), RuleError::NotAvailable, "mission region or navigation");
    ctx.push_return(ScriptValue::Bool(false))
}

fn host_add_death_region(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    check_string_id(ctx, 0)?;
    record_rule_error(ctx.host_mut(), RuleError::NotAvailable, "mission region or navigation");
    ctx.push_return(ScriptValue::Bool(false))
}

// ---------------------------------------------------------------------------
// Group 5: Events & callbacks
// ---------------------------------------------------------------------------

fn register_listener(
    ctx: &mut ScriptCallCtx<'_, HostState>,
    cb_idx: usize,
    family: ListenerFamily,
    object: Option<(u8, u32, u32)>,
    timer_id: u32,
) -> Result<(), ScriptError> {
    charge(ctx)?;
    let cb = ctx.check_function(cb_idx)?;
    let (vm, host) = ctx.split();
    let vref = vm.create_ref(cb)?;
    match host.add_listener(family, vref, object, timer_id) {
        Some(id) => ctx.push_return(ScriptValue::Number(id as f32)),
        None => {
            vm.release_ref(vref);
            record_rule_error(host, RuleError::LimitReached, "On* listener registration");
            ctx.push_return(ScriptValue::Nil)
        }
    }
}

fn host_on_character_death(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    register_listener(ctx, 0, ListenerFamily::CharacterDeath, None, 0)
}

fn host_on_ticket_count_change(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    register_listener(ctx, 0, ListenerFamily::TicketCountChange, None, 0)
}

fn host_on_team_points_change(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    register_listener(ctx, 0, ListenerFamily::TeamPointsChange, None, 0)
}

fn host_on_timer_elapse(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    let Some(timer_id) = resolve_timer_id(ctx, 1) else {
        return ctx.push_return(ScriptValue::Nil);
    };
    register_listener(ctx, 0, ListenerFamily::TimerElapse, None, timer_id)
}

fn register_object(ctx: &mut ScriptCallCtx<'_, HostState>, family: ListenerFamily) -> Result<(), ScriptError> {
    let Some(object) = resolve_authoritative_object(ctx, 1, "object listener")? else {
        return ctx.push_return(ScriptValue::Nil);
    };
    register_listener(ctx, 0, family, Some(crate::battle::rules::objects::key(&object)), 0)
}

fn host_on_object_kill_name(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    register_object(ctx, ListenerFamily::ObjectKill)
}

fn host_on_object_damage_name(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    register_object(ctx, ListenerFamily::ObjectDamage)
}

fn host_on_object_respawn_name(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    register_object(ctx, ListenerFamily::ObjectRespawn)
}

fn release_listener_family(
    ctx: &mut ScriptCallCtx<'_, HostState>,
    family: ListenerFamily,
) -> Result<(), ScriptError> {
    charge(ctx)?;
    // Silently ignore nil or invalid handles.
    let ScriptValue::Number(n) = ctx.arg(0) else {
        return Ok(());
    };
    let Some(id) = lua_to_exact_handle(n) else {
        return Ok(());
    };
    let (vm, host) = ctx.split();
    match host.remove_listener_of_family(id, Some(family)) {
        Ok(Some(vref)) => { vm.release_ref(vref); }
        Ok(None) => {}
        Err(actual) => {
            host.diagnostics.record_error(
                host.tick,
                host.phase,
                Some(id),
                DiagnosticKind::InvalidArgument,
                format!("mismatched Release*: expected {family:?}, handle is {actual:?}"),
            );
        }
    }
    Ok(())
}

fn host_release_character_death(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    release_listener_family(ctx, ListenerFamily::CharacterDeath)
}

fn host_release_ticket_count_change(
    ctx: &mut ScriptCallCtx<'_, HostState>,
) -> Result<(), ScriptError> {
    release_listener_family(ctx, ListenerFamily::TicketCountChange)
}

fn host_release_timer_elapse(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    release_listener_family(ctx, ListenerFamily::TimerElapse)
}

fn host_release_team_points_change(
    ctx: &mut ScriptCallCtx<'_, HostState>,
) -> Result<(), ScriptError> {
    release_listener_family(ctx, ListenerFamily::TeamPointsChange)
}

fn host_release_object_kill(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    release_listener_family(ctx, ListenerFamily::ObjectKill)
}

fn host_release_object_damage(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    release_listener_family(ctx, ListenerFamily::ObjectDamage)
}

fn host_release_object_respawn(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    release_listener_family(ctx, ListenerFamily::ObjectRespawn)
}

// ---------------------------------------------------------------------------
// Group 6: Timers (authoritative deadlines backed by RuleActions)
// ---------------------------------------------------------------------------

fn resolve_timer_id(ctx: &ScriptCallCtx<'_, HostState>, idx: usize) -> Option<u32> {
    match ctx.arg(idx) {
        ScriptValue::Number(n) => {
            let id = lua_to_exact_handle(n)?;
            ctx.host().timer(id).map(|_| id)
        }
        ScriptValue::String(sid) => {
            let raw = ctx.vm().string_bytes(sid)?;
            ctx.host().find_timer_by_bytes(raw)
        }
        _ => None,
    }
}

fn arm_rules_timer(host: &mut HostState, id: u32) -> bool {
    let Some(timer) = host.timer(id) else {
        return false;
    };
    let rules_id = timer.rules_id();
    let remaining = timer.remaining;
    let rate = timer.rate;
    if rate <= 0.0 {
        let _ = with_actions(|a| a.stop_timer(rules_id));
        return true;
    }
    let wall_secs = remaining / rate;
    match with_actions(|a| a.start_timer(rules_id, wall_secs)) {
        Some(true) => true,
        Some(false) => {
            record_rule_error(host, RuleError::LimitReached, "StartTimer");
            false
        }
        None => true,
    }
}

fn host_create_timer(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let name_bytes = check_bytes(ctx, 0)?;
    match ctx.host_mut().create_timer(name_bytes) {
        Some(id) => ctx.push_return(ScriptValue::Number(id as f32)),
        None => {
            record_rule_error(ctx.host_mut(), RuleError::LimitReached, "CreateTimer");
            ctx.push_return(ScriptValue::Nil)
        }
    }
}

fn host_find_timer(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    // Documented API accepts either a string timer name OR a numeric timer handle.
    match resolve_timer_id(ctx, 0) {
        Some(id) => ctx.push_return(ScriptValue::Number(id as f32)),
        None => ctx.push_return(ScriptValue::Nil),
    }
}

fn host_set_timer_value(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let Some(id) = resolve_timer_id(ctx, 0) else {
        return Ok(());
    };
    let seconds = ctx.check_number(1)?;
    if let Some(secs) = lua_to_finite_f64(seconds) {
        let clamped = if secs > 0.0 { secs } else { 0.0 };
        let host = ctx.host_mut();
        let was_running = if let Some(timer) = host.timer_mut(id) {
            timer.remaining = clamped;
            timer.arm_gen = timer.arm_gen.wrapping_add(1);
            timer.running
        } else {
            false
        };
        host.invalidate_pending_timer(id);
        if was_running && !arm_rules_timer(host, id) {
            if let Some(timer) = host.timer_mut(id) {
                timer.running = false;
            }
        }
    }
    Ok(())
}

fn host_get_timer_value(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let rem = resolve_timer_id(ctx, 0)
        .and_then(|id| ctx.host().timer(id))
        .map_or(0.0, |t| t.remaining);
    ctx.push_return(ScriptValue::Number(f64_to_lua(rem)))
}

fn host_set_timer_rate(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let Some(id) = resolve_timer_id(ctx, 0) else {
        return Ok(());
    };
    let rate = ctx.check_number(1)?;
    if let Some(r) = lua_to_finite_f64(rate) {
        let host = ctx.host_mut();
        let was_running = if let Some(timer) = host.timer_mut(id) {
            timer.rate = r;
            timer.arm_gen = timer.arm_gen.wrapping_add(1);
            timer.running
        } else {
            false
        };
        host.invalidate_pending_timer(id);
        if was_running && !arm_rules_timer(host, id) {
            if let Some(timer) = host.timer_mut(id) {
                timer.running = false;
            }
        }
    }
    Ok(())
}

fn host_start_timer(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let Some(id) = resolve_timer_id(ctx, 0) else {
        return Ok(());
    };
    let host = ctx.host_mut();
    host.invalidate_pending_timer(id);
    if !arm_rules_timer(host, id) {
        return Ok(());
    }
    let tick = host.tick;
    if let Some(timer) = host.timer_mut(id) {
        timer.running = true;
        timer.started_tick = tick;
        timer.arm_gen = timer.arm_gen.wrapping_add(1);
    }
    Ok(())
}

fn host_stop_timer(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let Some(id) = resolve_timer_id(ctx, 0) else {
        return Ok(());
    };
    let host = ctx.host_mut();
    if let Some(timer) = host.timer_mut(id) {
        let rules_id = timer.rules_id();
        timer.running = false;
        timer.arm_gen = timer.arm_gen.wrapping_add(1);
        let _ = with_actions(|a| a.stop_timer(rules_id));
    }
    host.invalidate_pending_timer(id);
    Ok(())
}

fn host_destroy_timer(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let Some(id) = resolve_timer_id(ctx, 0) else {
        return Ok(());
    };
    let host = ctx.host_mut();
    if let Some(timer) = host.timer_mut(id) {
        let rules_id = timer.rules_id();
        timer.alive = false;
        timer.running = false;
        timer.arm_gen = timer.arm_gen.wrapping_add(1);
        let _ = with_actions(|a| a.stop_timer(rules_id));
        if host.hud_timer == Some(id) {
            host.hud_timer = None;
        }
    }
    host.invalidate_pending_timer(id);
    Ok(())
}

fn host_show_timer(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    if ctx.arg(0).is_nil() {
        let host = ctx.host_mut();
        host.hud_timer = None;
        host.push_notification(ScriptNotification::ShowTimer { timer_name: None });
        return Ok(());
    }
    if let Some(id) = resolve_timer_id(ctx, 0) {
        let name = ctx.host().timer(id).map(super::state::ScriptTimer::name_lossy);
        let host = ctx.host_mut();
        host.hud_timer = Some(id);
        host.push_notification(ScriptNotification::ShowTimer { timer_name: name });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Group 7: Objectives & scoring (authoritative via RuleActions)
// ---------------------------------------------------------------------------

fn host_add_mission_objective(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let team = check_int(ctx, 0)?;
    let text_key = check_string(ctx, 1)?;
    let popup_key = opt_string(ctx, 2)?;
    let host = ctx.host_mut();
    if host.objectives.len() >= MAX_OBJECTIVES {
        record_rule_error(host, RuleError::LimitReached, "AddMissionObjective");
        return Ok(());
    }
    host.objectives.push(ObjectiveEntry {
        team,
        text_key: text_key.clone(),
        popup_key: popup_key.clone(),
        active: false,
        completed: false,
    });
    host.push_notification(ScriptNotification::AddMissionObjective {
        team,
        text_key,
        popup_key,
    });
    Ok(())
}

fn host_activate_objective(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let text_key = check_string(ctx, 0)?;
    let host = ctx.host_mut();
    for obj in &mut host.objectives {
        if obj.text_key == text_key {
            obj.active = true;
        }
    }
    host.push_notification(ScriptNotification::ActivateObjective { text_key });
    Ok(())
}

fn host_complete_objective(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let text_key = check_string(ctx, 0)?;
    let host = ctx.host_mut();
    for obj in &mut host.objectives {
        if obj.text_key == text_key {
            obj.active = false;
            obj.completed = true;
        }
    }
    host.push_notification(ScriptNotification::CompleteObjective { text_key });
    Ok(())
}

fn host_set_flag_gameplay_type(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let mode = check_string(ctx, 0)?;
    ctx.host_mut().match_flags.flag_gameplay_type = mode;
    Ok(())
}

fn host_set_team_points(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let Some(team) = playable_team(ctx, "SetTeamPoints")? else { return Ok(()) };
    let points = check_int(ctx, 1)?.max(0) as u32;
    if matches!(
        ctx.host().phase,
        ScriptPhase::Chunk | ScriptPhase::PreInit | ScriptPhase::Init
    ) {
        ctx.host_mut().team_setup_mut(team).initial_score = Some(points);
        return Ok(());
    }

    let result = with_actions(|a| {
        let prev = a.team_points(f64::from(team)).ok();
        let res = a.set_team_points(f64::from(team), points);
        (prev, res)
    });
    match result {
        Some((prev, Ok(()))) => {
            if let Some(slot) = team_slot(team) {
                ctx.host_mut().cached_scores[slot] = points;
            }
            if prev != Some(points) {
                ctx.host_mut().emitted_points_seq =
                    ctx.host().emitted_points_seq.saturating_add(1);
            }
        }
        Some((_, Err(err))) => record_rule_error(ctx.host_mut(), err, "SetTeamPoints"),
        None => record_rule_error(ctx.host_mut(), RuleError::NotAvailable, "SetTeamPoints"),
    }
    Ok(())
}

fn host_add_team_points(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let Some(team) = playable_team(ctx, "AddTeamPoints")? else { return Ok(()) };
    let delta = check_int(ctx, 1)?;
    if matches!(
        ctx.host().phase,
        ScriptPhase::Chunk | ScriptPhase::PreInit | ScriptPhase::Init
    ) {
        let setup = ctx.host_mut().team_setup_mut(team);
        let cur = setup.initial_score.unwrap_or(0);
        setup.initial_score = Some(cur.saturating_add_signed(delta));
        return Ok(());
    }

    // A death listener can grant points to the killer's team. Other team
    // changes have no individual recipient. Space destruction has its own awards.
    let host = ctx.host();
    let scorer = host.active_death
        .filter(|d| delta != 0 && !host.match_flags.space_assault_enabled && d.killer_team == Some(team))
        .and_then(|d| host.character(d.killer_handle?))
        .map(|c| c.sim_ref);
    let result = with_actions(|a| {
        let prev = a.team_points(f64::from(team)).ok();
        let res = match scorer {
            Some(character) => a.add_scored_team_points(f64::from(team), delta, character),
            None => a.add_team_points(f64::from(team), delta),
        };
        (prev, res)
    });
    match result {
        Some((prev, Ok(next))) => {
            if let Some(slot) = team_slot(team) {
                ctx.host_mut().cached_scores[slot] = next;
            }
            if prev != Some(next) {
                ctx.host_mut().emitted_points_seq =
                    ctx.host().emitted_points_seq.saturating_add(1);
            }
        }
        Some((_, Err(err))) => record_rule_error(ctx.host_mut(), err, "AddTeamPoints"),
        None => record_rule_error(ctx.host_mut(), RuleError::NotAvailable, "AddTeamPoints"),
    }
    Ok(())
}

fn host_get_team_points(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let team = check_int(ctx, 0)?;
    if matches!(
        ctx.host().phase,
        ScriptPhase::Chunk | ScriptPhase::PreInit | ScriptPhase::Init
    ) {
        let score = ctx
            .host()
            .team_setup(team)
            .and_then(|t| t.initial_score)
            .unwrap_or(0);
        return ctx.push_return(ScriptValue::Number(score as f32));
    }
    let score = match with_actions(|a| a.team_points(f64::from(team))) {
        Some(Ok(pts)) => pts,
        Some(Err(err)) => {
            record_rule_error(ctx.host_mut(), err, "GetTeamPoints");
            0
        }
        None => 0,
    };
    ctx.push_return(ScriptValue::Number(score as f32))
}

fn declare_result(host: &mut HostState, outcome: Outcome, recorded: BattleOutcome, operation: &str) {
    if host.phase == ScriptPhase::PostLoad {
        let valid = match outcome {
            Outcome::Winner(t) | Outcome::Loser(t) => t == f64::from(host.config.own_team) || t == f64::from(host.config.foe_team),
            Outcome::Draw => true,
        };
        if valid && host.setup_result.is_none() {
            host.setup_result = Some((outcome, recorded));
        } else {
            record_rule_error(host, RuleError::InvalidValue, operation);
        }
        return;
    }
    match with_actions(|a| a.finish(outcome)) {
        Some(Ok(())) => host.outcome = Some(recorded),
        Some(Err(err)) => record_rule_error(host, err, operation),
        None => record_rule_error(host, RuleError::NotAvailable, operation),
    }
}

fn host_mission_victory(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    ctx.host_mut().attempted_result = true;
    charge(ctx)?;
    match ctx.arg(0) {
        ScriptValue::Table(tid) => {
            let teams = table_ints(ctx, tid)?;
            let mut distinct = Vec::new();
            for t in teams {
                if !distinct.contains(&t) {
                    distinct.push(t);
                }
            }
            let (outcome, recorded) = match distinct.as_slice() {
                [] => {
                    record_rule_error(ctx.host_mut(), RuleError::InvalidValue, "MissionVictory");
                    return Ok(());
                }
                [only] => (Outcome::Winner(f64::from(*only)), BattleOutcome::Victory(*only)),
                _ => {
                    // Validate all listed teams are valid admitted armies before drawing.
                    let own = ctx.host().config.own_team;
                    let foe = ctx.host().config.foe_team;
                    if !distinct.iter().all(|&t| t == own || t == foe) {
                        record_rule_error(ctx.host_mut(), RuleError::InvalidValue, "MissionVictory");
                        return Ok(());
                    }
                    (Outcome::Draw, BattleOutcome::Draw(distinct))
                }
            };
            declare_result(ctx.host_mut(), outcome, recorded, "MissionVictory");
        }
        _ => {
            let winner = check_int(ctx, 0)?;
            declare_result(ctx.host_mut(), Outcome::Winner(winner.into()), BattleOutcome::Victory(winner), "MissionVictory");
        }
    }
    Ok(())
}

fn host_mission_defeat(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    ctx.host_mut().attempted_result = true;
    charge(ctx)?;
    let loser = check_int(ctx, 0)?;
    declare_result(ctx.host_mut(), Outcome::Loser(loser.into()), BattleOutcome::Defeat(loser), "MissionDefeat");
    Ok(())
}

fn host_get_assault_score_limit(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let admitted = with_actions(|a| a.score_limit()).flatten().or(ctx.host().cached_score_limit);
    let limit = ctx.host().config.effective_assault_score_limit(admitted) as f32;
    ctx.push_return(ScriptValue::Number(limit))
}

fn host_get_hunt_score_limit(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let limit = ctx.host().config.effective_hunt_score_limit() as f32;
    ctx.push_return(ScriptValue::Number(limit))
}

fn host_get_hunt_max_time_limit(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let limit = ctx.host().config.effective_hunt_max_time_limit() as f32;
    ctx.push_return(ScriptValue::Number(limit))
}

fn host_get_uber_score_limit(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let limit = ctx.host().config.effective_uber_score_limit() as f32;
    ctx.push_return(ScriptValue::Number(limit))
}

fn host_set_uber_score_limit(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let limit = check_int(ctx, 0)?.max(0) as u32;
    ctx.host_mut().config.uber_score_limit = Some(limit);
    Ok(())
}

fn host_show_hunt_score_limit(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let mode = check_int(ctx, 0)?;
    let host = ctx.host_mut();
    host.match_flags.hud_score_limit_mode = Some(mode);
    host.push_notification(ScriptNotification::ShowHuntScoreLimit { mode });
    Ok(())
}

fn host_is_campaign(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let flag = ctx.host().config.is_campaign;
    ctx.push_return(ScriptValue::Bool(flag))
}

fn host_get_world_filename(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let name = ctx.host().config.world_filename.clone();
    push_str(ctx, &name)
}

// ---------------------------------------------------------------------------
// Group 8: Messages & HUD
// ---------------------------------------------------------------------------

fn host_show_team_points(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let team = check_int(ctx, 0)?;
    let enabled = arg_flag(ctx, 1, true);
    if let Some(i) = team_slot(team) { ctx.host_mut().team_points_visible[i] = enabled; }
    ctx.host_mut()
        .push_notification(ScriptNotification::ShowTeamPoints { team, enabled });
    Ok(())
}

fn host_show_message_text(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let key = label(ctx, 0)?;
    let team = if ctx.arg(1).is_nil() {
        None
    } else {
        Some(check_int(ctx, 1)?)
    };
    ctx.host_mut()
        .push_notification(ScriptNotification::ShowMessageText { key, team });
    Ok(())
}

fn host_add_mission_hint(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let key = check_string(ctx, 0)?;
    ctx.host_mut()
        .push_notification(ScriptNotification::AddMissionHint { key });
    Ok(())
}

fn host_map_hide_command_posts(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let hide = arg_flag(ctx, 0, true);
    ctx.host_mut().command_posts_hidden = hide;
    ctx.host_mut()
        .push_notification(ScriptNotification::MapHideCommandPosts { hide });
    Ok(())
}

fn host_disable_small_map_minimap(
    ctx: &mut ScriptCallCtx<'_, HostState>,
) -> Result<(), ScriptError> {
    charge(ctx)?;
    ctx.host_mut().minimap_disabled = true;
    ctx.host_mut()
        .push_notification(ScriptNotification::DisableSmallMapMiniMap);
    Ok(())
}

fn host_get_num_cameras(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let count = ctx.host().config.num_cameras.max(1) as f32;
    ctx.push_return(ScriptValue::Number(count))
}

// ---------------------------------------------------------------------------
// Group 9: Sound & voice-over
// ---------------------------------------------------------------------------

fn host_open_audio_stream(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    setup_only(ctx)?;
    let lvl_path = check_string(ctx, 0)?;
    let stream_id = check_string(ctx, 1)?;
    let host = ctx.host_mut();
    if host.setup.audio_streams.len() >= MAX_AUDIO_STREAMS
        || host.next_audio_stream_id > MAX_EXACT_LUA_HANDLE
    {
        record_rule_error(host, RuleError::LimitReached, "OpenAudioStream");
        return ctx.push_return(ScriptValue::Number(0.0));
    }
    let handle = host.next_audio_stream_id;
    host.next_audio_stream_id += 1;
    host.setup.audio_streams.push(AudioStreamRecord {
        handle,
        lvl_path,
        stream_id,
        appended: Vec::new(),
    });
    ctx.push_return(ScriptValue::Number(handle as f32))
}

fn host_audio_stream_append_segments(
    ctx: &mut ScriptCallCtx<'_, HostState>,
) -> Result<(), ScriptError> {
    charge(ctx)?;
    setup_only(ctx)?;
    let lvl_path = check_string(ctx, 0)?;
    let stream_id = check_string(ctx, 1)?;
    let handle = match ctx.arg(2) {
        ScriptValue::Number(n) => lua_to_exact_handle(n).unwrap_or(0),
        _ => 0,
    };
    if handle > 0 {
        if let Some(stream) = ctx
            .host_mut()
            .setup
            .audio_streams
            .iter_mut()
            .find(|s| s.handle == handle && s.lvl_path == lvl_path)
        {
            stream.appended.push((lvl_path, stream_id));
            return ctx.push_return(ScriptValue::Number(handle as f32));
        }
    }
    // Documented API returns the stream handle on success or 0 on failure.
    ctx.push_return(ScriptValue::Number(0.0))
}

fn host_broadcast_voice_over(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let sound_id = label(ctx, 0)?;
    let team = if ctx.arg(1).is_nil() {
        None
    } else {
        Some(check_int(ctx, 1)?)
    };
    let host = ctx.host_mut();
    let dur = f64::from(host.config.voice_over_duration_ms) / 1000.0;
    if dur > 0.0 {
        if let Some((_,time))=host.active_voices.iter_mut().find(|(name,_)| *name == sound_id) { *time=dur; }
        else if host.active_voices.len() < 32 { host.active_voices.push((sound_id.clone(),dur)); }
        else { return Err(ScriptError::ResourceLimit("active mission voices")); }
    }
    host.push_notification(ScriptNotification::BroadcastVoiceOver { sound_id, team });
    Ok(())
}

fn host_set_bleeding_vo(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    setup_only(ctx)?;
    let team = check_int(ctx, 0)?;
    let losing_team = check_int(ctx, 1)?;
    let sound = check_string(ctx, 2)?;
    let state = opt_int(ctx, 3, 0)?;
    ctx.host_mut()
        .setup
        .audio_config
        .bleeding_vo
        .push((team, losing_team, sound, state));
    Ok(())
}

fn host_set_low_reinforcements_vo(
    ctx: &mut ScriptCallCtx<'_, HostState>,
) -> Result<(), ScriptError> {
    charge(ctx)?;
    setup_only(ctx)?;
    let team = check_int(ctx, 0)?;
    let losing_team = check_int(ctx, 1)?;
    let sound = check_string(ctx, 2)?;
    let threshold = ctx.opt_number(3, 0.0).ok().and_then(lua_to_finite_f64).unwrap_or(0.0);
    let state = opt_int(ctx, 4, 0)?;
    ctx.host_mut()
        .setup
        .audio_config
        .low_reinforcements_vo
        .push((team, losing_team, sound, threshold, state));
    Ok(())
}

fn host_set_out_of_bounds_vo(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    setup_only(ctx)?;
    let team = check_int(ctx, 0)?;
    let sound = check_string(ctx, 1)?;
    ctx.host_mut()
        .setup
        .audio_config
        .out_of_bounds_vo
        .push((team, sound));
    Ok(())
}

fn host_set_ambient_music(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    setup_only(ctx)?;
    let team = check_int(ctx, 0)?;
    let threshold = ctx.opt_number(1, 1.0).ok().and_then(lua_to_finite_f64).unwrap_or(1.0);
    let sound = check_string(ctx, 2)?;
    let start_end = opt_int(ctx, 3, 0)?;
    let mode = opt_int(ctx, 4, 0)?;
    ctx.host_mut()
        .setup
        .audio_config
        .ambient_music
        .push((team, threshold, sound, start_end, mode));
    Ok(())
}

fn host_set_victory_music(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    setup_only(ctx)?;
    let team = check_int(ctx, 0)?;
    let sound = check_string(ctx, 1)?;
    ctx.host_mut()
        .setup
        .audio_config
        .victory_music
        .push((team, sound));
    Ok(())
}

fn host_set_defeat_music(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    setup_only(ctx)?;
    let team = check_int(ctx, 0)?;
    let sound = check_string(ctx, 1)?;
    ctx.host_mut()
        .setup
        .audio_config
        .defeat_music
        .push((team, sound));
    Ok(())
}

fn host_set_sound_effect(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    setup_only(ctx)?;
    let event = check_string(ctx, 0)?;
    let sound = check_string(ctx, 1)?;
    let team = if ctx.arg(2).is_nil() { None } else { Some(check_int(ctx,2)?) };
    ctx.host_mut()
        .setup
        .audio_config
        .sound_effects
        .push((event, sound, team));
    Ok(())
}

fn host_set_doppler_factor(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let n = ctx.check_number(0)?;
    if let Some(v) = lua_to_finite_f64(n) {
        ctx.host_mut().setup.audio_config.doppler_factor = Some(v);
    }
    Ok(())
}

fn host_scale_sound_parameter(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    setup_only(ctx)?;
    let target = check_string(ctx, 0)?;
    let param = check_string(ctx, 1)?;
    let scale = ctx.opt_number(2, 1.0).ok().and_then(lua_to_finite_f64).unwrap_or(1.0);
    ctx.host_mut()
        .setup
        .audio_config
        .scaled_parameters
        .push((target, param, scale));
    Ok(())
}

fn host_set_attacking_team(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let team = check_int(ctx, 0)?;
    ctx.host_mut().setup.attacking_team = Some(team);
    Ok(())
}

fn host_enable_hero_music(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let enabled = arg_flag(ctx, 0, true);
    ctx.host_mut().match_flags.hero_music_enabled = enabled;
    Ok(())
}

fn host_enable_hero_vo(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let enabled = arg_flag(ctx, 0, true);
    ctx.host_mut().match_flags.hero_vo_enabled = enabled;
    Ok(())
}

fn host_audio_stream_complete(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    // Documented API returns integer 1 when complete, 0 while still playing.
    let id = label(ctx,0)?;
    let done = if ctx.host().active_voices.iter().any(|(name,_)| *name == id) { 0.0 } else { 1.0 };
    ctx.push_return(ScriptValue::Number(done))
}

// ---------------------------------------------------------------------------
// Group 10: Space Assault
// ---------------------------------------------------------------------------

fn host_add_landing_region(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let name = check_string(ctx, 0)?;
    if matches!(ctx.host().phase, ScriptPhase::PostLoad | ScriptPhase::Tick) {
        let result = with_actions(|actions| actions.add_landing_region(&name))
            .unwrap_or(Err(RuleError::NotAvailable));
        if let Err(error) = result {
            record_rule_error(ctx.host_mut(), error, "AddLandingRegion");
            return ctx.push_return(ScriptValue::Bool(false));
        }
    } else {
        let regions = &mut ctx.host_mut().setup.landing_regions;
        if !regions.iter().any(|region| region.eq_ignore_ascii_case(&name)) {
            regions.push(name);
        }
    }
    ctx.push_return(ScriptValue::Bool(true))
}

fn host_space_assault_enable(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    charge(ctx)?;
    let enabled = arg_flag(ctx, 0, true);
    ctx.host_mut().match_flags.space_assault_enabled = enabled;
    Ok(())
}

fn host_space_teams(ctx:&mut ScriptCallCtx<'_, HostState>)->Result<(),ScriptError> {
    charge(ctx)?;
    let teams = [check_int(ctx,0)?,check_int(ctx,1)?];
    if teams.iter().any(|&t|team_slot(t).is_none()) || teams[0]==teams[1] {
        record_rule_error(ctx.host_mut(),RuleError::InvalidValue,"SpaceAssaultSetupTeamNumbers");
        return ctx.push_return(ScriptValue::Bool(false));
    }
    ctx.host_mut().space_teams=Some(teams);
    Ok(())
}

fn host_space_assault_setup_bitmaps(
    ctx: &mut ScriptCallCtx<'_, HostState>,
) -> Result<(), ScriptError> {
    charge(ctx)?;
    setup_only(ctx)?;
    if ctx.arg_count()!=6 { return Err(ScriptError::runtime("expected six Space Assault bitmap names")); }
    let mut bitmaps = Vec::with_capacity(6);
    for i in 0..ctx.arg_count() {
        bitmaps.push(check_string(ctx,i)?);
    }
    ctx.host_mut().space_assault_bitmaps = bitmaps;
    Ok(())
}

fn host_space_assault_add_critical_system(
    ctx: &mut ScriptCallCtx<'_, HostState>,
) -> Result<(), ScriptError> {
    charge(ctx)?;
    setup_only(ctx)?;
    let Some(object) = resolve_authoritative_object(ctx, 0, "SpaceAssaultAddCriticalSystem")? else { return ctx.push_return(ScriptValue::Bool(false)); };
    let name = check_string(ctx, 0)?;
    let points = check_int(ctx, 1)?;
    let hud_u = ctx.opt_number(2, -1.0)?;
    let hud_v = ctx.opt_number(3, -1.0)?;
    let show_marker = arg_flag(ctx, 4, true);
    let host = ctx.host_mut();
    let row = CriticalSystemRecord { object, name, points, hud_u, hud_v, show_marker };
    if let Some(existing) = host.critical_systems.iter_mut().find(|s| s.object == row.object) {
        *existing = row;
    } else if host.critical_systems.len() < MAX_CRITICAL_SYSTEMS {
        host.critical_systems.push(row);
    } else {
        record_rule_error(host, RuleError::LimitReached, "SpaceAssaultAddCriticalSystem");
        return ctx.push_return(ScriptValue::Bool(false));
    }
    Ok(())
}

fn host_space_assault_link_critical_systems(
    ctx: &mut ScriptCallCtx<'_, HostState>,
) -> Result<(), ScriptError> {
    charge(ctx)?;
    setup_only(ctx)?;
    let tbl = ctx.check_table(0)?;
    let names = table_strings(ctx, tbl)?;
    let host = ctx.host_mut();
    if names.len() < 2 || host.linked_critical_systems.len() >= MAX_CRITICAL_SYSTEMS ||
        names.iter().any(|name| !host.critical_systems.iter().any(|s| s.name.eq_ignore_ascii_case(name))) {
        record_rule_error(host, RuleError::InvalidValue, "SpaceAssaultLinkCriticalSystems");
        return ctx.push_return(ScriptValue::Bool(false));
    }
    for child in &names[1..] {
        if let Some(row) = host.critical_systems.iter_mut().find(|s| s.name.eq_ignore_ascii_case(child)) { row.show_marker = false; }
    }
    host.linked_critical_systems.push(names);
    Ok(())
}

fn host_space_assault_get_score_limit(
    ctx: &mut ScriptCallCtx<'_, HostState>,
) -> Result<(), ScriptError> {
    charge(ctx)?;
    let admitted = with_actions(|a| a.score_limit()).flatten().or(ctx.host().cached_score_limit);
    let limit = ctx
        .host()
        .config
        .effective_assault_score_limit(admitted) as f32;
    ctx.push_return(ScriptValue::Number(limit))
}

fn host_add_space_assault_destroy_points(
    ctx: &mut ScriptCallCtx<'_, HostState>,
) -> Result<(), ScriptError> {
    charge(ctx)?;
    // Ignore nil killer so unattributed or script-initiated object kills do not award points.
    let ScriptValue::Number(killer_num) = ctx.arg(0) else {
        return Ok(());
    };
    let Some(killer_handle) = lua_to_exact_handle(killer_num) else {
        return Ok(());
    };
    let active = ctx.host().active_object.clone();
    let credit = if let Some((_,_,Some(c),Some(id)))=&active {
        (*id==killer_handle).then_some(*c)
    } else {None};
    let credit = credit.or_else(|| {
        let character=ctx.host().character(killer_handle)?.sim_ref;
        with_actions(|a| Some(super::super::rules::awards::Credit { character:Some(character),
            team:a.character_team(character).ok()? as i32,human:a.character_is_human(character).ok()? })).flatten()
    });
    let Some(credit)=credit else{return Ok(())};
    let Some(handle) = resolve_authoritative_object(ctx, 1, "AddSpaceAssaultDestroyPoints")? else { return Ok(()); };
    use crate::battle::rules::objects::key;
    let named=key(&handle);
    let Some(points)=ctx.host().critical_systems.iter().find(|s| key(&s.object)==named).map(|s|s.points).filter(|n|*n!=0) else {return Ok(())};
    let team=active.filter(|(object,..)|key(object)==named).map(|(_,team,..)|team)
        .or_else(||with_actions(|a|a.object_team(&handle).ok()).flatten().map(|t|t as i32));
    if let Some(team)=team {with_actions(|a|ctx.host_mut().credit_points(a,credit,team,points));}
    Ok(())
}

#[inline]
pub(crate) fn team_slot(team: i32) -> Option<usize> {
    if team >= 1 && (team as usize) < TEAM_COUNT {
        Some(team as usize)
    } else {
        None
    }
}
