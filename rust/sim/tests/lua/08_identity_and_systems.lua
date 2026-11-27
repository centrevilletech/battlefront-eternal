-- Original test fixture for character identity (human/bot, launch team,
-- nil victim, attributed killer without ref) and object identity/life
-- (missing, ambiguous, enemy/friendly/paired/duplicate/rebuilt-and-rekilled).

function ScriptPostLoad()
    -- Missing and ambiguous objects must never be invented.
    assert(GetObjectTeam("no_such_bldg") == 0, "missing object returns team 0")
    assert(IsObjectAlive("no_such_bldg") == false, "missing object is not alive")
    SetProperty("no_such_bldg", "Team", 1)
    assert(GetObjectTeam("dup_bldg") == 0, "ambiguous object returns team 0")

    SpaceAssaultEnable(true)
    SpaceAssaultAddCriticalSystem("crit_enemy", 10, 0.2, 0.2, true)
    SpaceAssaultAddCriticalSystem("crit_friendly", 10, 0.3, 0.3, true)
    SpaceAssaultAddCriticalSystem("crit_paired", 10, 0.4, 0.4, true)

    local function award_by_name(attacker, target_obj)
        local target_label = GetEntityName(target_obj)
        AddSpaceAssaultDestroyPoints(attacker, target_label)
    end

    local first_life_ud = nil
    OnObjectKillName(function(obj, killer)
        -- Two explicit awards share the same impact credit.
        AddSpaceAssaultDestroyPoints(killer, obj)
        award_by_name(killer, obj)
        if first_life_ud == nil then
            first_life_ud = obj
            if IsObjectAlive("crit_paired") then
                KillObject("crit_paired")
            end
        end
    end, "crit_enemy")

    OnObjectKillName(function(obj, killer)
        award_by_name(killer, obj)
    end, "crit_friendly")

    OnObjectKillName(function(obj, killer)
        award_by_name(killer, obj)
    end, "crit_paired")

    -- Timer at 0.25s repairs crit_enemy via authoritative SetProperty("CurHealth", 500)
    -- and then attempts KillObject(first_life_ud), which must be refused as StaleHandle.
    local repair_clock = CreateTimer("repair_clock")
    SetTimerValue(repair_clock, 0.25)
    StartTimer(repair_clock)
    OnTimerElapse(function()
        SetProperty("crit_enemy", "CurHealth", 500)
        assert(IsObjectAlive("crit_enemy") == true, "repaired building must be alive")
        if first_life_ud ~= nil then
            KillObject(first_life_ud)
            assert(IsObjectAlive("crit_enemy") == true, "stale life handle must not kill rebuilt object")
        end
    end, repair_clock)

    -- Character death identity encoding into team 2 score:
    -- +1 for every delivered OnCharacterDeath
    -- +10 if killer has team 1
    -- +100 if IsCharacterHuman(killer) is true
    OnCharacterDeath(function(victim, killer)
        AddTeamPoints(2, 1)
        if killer ~= nil then
            if GetCharacterTeam(killer) == 1 then
                AddTeamPoints(2, 10)
            end
            if IsCharacterHuman(killer) then
                AddTeamPoints(2, 100)
            end
        end
    end)
end
