-- Original test fixture covering documented API return types, audio
-- completion timing, typed AI goal targets, family-checked releases,
-- strict pcall unbound counting, sequenced notification ring eviction,
-- and raw non-UTF-8 byte isolation.

function ScriptInit()
    -- Camera shot arity check: 6 args must fail, 7 args must succeed.
    local bad_cam = pcall(AddCameraShot, 0, 0, 0, 1, 2, 3)
    assert(bad_cam == false, "6-arg AddCameraShot must fail")
    AddCameraShot(0, 0, 0, 1, 10, 20, 30)

    -- Audio stream append returns handle on valid stream and 0 on invalid stream.
    local s = OpenAudioStream("sound\\api.lvl", "base_vo")
    assert(type(s) == "number" and s > 0, "OpenAudioStream returns positive handle")
    assert(AudioStreamAppendSegments("sound\\api.lvl", "seg_a", s) == s, "append returns stream handle")
    assert(AudioStreamAppendSegments("sound\\api.lvl", "seg_bad", 9999) == 0, "bad append returns 0")

    assert(AudioStreamAppendSegments("sound\\elsewhere.lvl", "seg_a", s) == 0)
    assert(pcall(AddCameraShot, 1, 1, 1, 1, 2, 3, 4, 5) == false)
    -- Audio configuration recording.
    SetAmbientMusic(1, 0.8, "amb_track", 0, 1)
    SetVictoryMusic(1, "win_track")
    SetDefeatMusic(1, "lose_track")
    SetSoundEffect("UI", "confirm")
    SetBleedingVoiceOver(1, 1, "bleed_vo", 1)
    SetLowReinforcementsVoiceOver(1, 1, "low_vo", 0.2, 1)
    SetOutOfBoundsVoiceOver(1, "oob_vo")
    ScriptCB_SetDopplerFactor(0.75)
    ScaleSoundParameter("bus_fx", "Gain", 1.25)
    ShowObjectiveTextPopup("deprecated.popup")
end

function ScriptPostLoad()
    -- Strict-mode pcall on unknown function must still record unbound call in diagnostics.
    local ok_unbound = pcall(function()
        MissingApiProbe(123)
    end)
    assert(ok_unbound == false, "strict mode must throw on MissingApiProbe")

    -- Raw non-UTF-8 single byte 0xE9 must not collide with 2-byte UTF-8 0xC3 0xA9.
    local raw_one = string.char(233)
    local utf8_two = string.char(195, 169)
    assert(GetEntityName(raw_one) == raw_one, "GetEntityName must preserve exact raw bytes")
    local raw_t = CreateTimer(raw_one)
    assert(FindTimer(raw_one) == raw_t, "FindTimer must match exact raw bytes")
    assert(FindTimer(raw_t) == raw_t, "FindTimer must accept numeric timer handle")
    assert(FindTimer(utf8_two) == nil, "UTF-8 spelling must not alias raw single byte")
    assert(FindTimer(99999) == nil, "unknown numeric timer handle returns nil")

    -- AudioStreamComplete returns integer 1 initially, then 0 while VO plays, then 1 after duration.
    assert(AudioStreamComplete("vo.start") == 1, "idle stream reports integer 1")
    BroadcastVoiceOver("vo.start", 1)
    assert(AudioStreamComplete("another") == 1)
    assert(AudioStreamComplete("vo.start") == 0, "active VO stream reports integer 0")

    -- Timer with family-checked release: calling ReleaseCharacterDeath on a timer listener
    -- must NOT remove it; timer still fires and checks AudioStreamComplete == 1.
    local check_t = CreateTimer("vo_check")
    SetTimerValue(check_t, 0.5)
    StartTimer(check_t)
    local timer_listener = OnTimerElapse(function()
        assert(AudioStreamComplete("vo.start") == 1, "completed VO stream must report integer 1")
        AddTeamPoints(1, 5)
    end, check_t)
    ReleaseCharacterDeath(timer_listener)

    -- Typed AI goal targets: character index on death, object userdata on object kill, and raw name.
    AddAIGoal(1, "CTFOffense", 25, "drop_zone", "flag_alpha")
    OnCharacterDeath(function(victim, killer)
        if killer ~= nil then
            AddAIGoal(1, "Defend", 40, killer)
        end
    end)

    local obj_listener = nil
    obj_listener = OnObjectKillName(function(obj, killer)
        AddAIGoal(1, "Destroy", 60, obj)
        ReleaseObjectKill(obj_listener)
        AddTeamPoints(1, 3)
    end, "goal_target")

    -- Overflow the 256-entry notification ring so the final cue stays retained with seq > 256.
    for i = 1, 270 do
        ShowMessageText("msg.spam", 1)
    end
    ShowMessageText("msg.final.cue", 1)
end
