-- Original test fixture for init-to-postload ticket boundary (3 -> 0 -> -1)
-- and invalid vs valid MissionVictory outcomes (0 refused, {1, 2} draw, 1 win).

function ScriptInit()
    SetTeamName(1, "alpha_init")
    SetUnitCount(1, 10)
    AddUnitClass(1, "cls_a", 2, 4)
    SetReinforcementCount(1, 3)
    SetReinforcementCount(1, 0)

    SetTeamName(2, "beta_init")
    SetUnitCount(2, 8)
    AddUnitClass(2, "cls_b", 1, 3)
    SetReinforcementCount(2, 2)
    SetSpawnDelay(6.0, 0.3)
end

function ScriptPostLoad()
    -- Switch team 1 to unlimited BEFORE registering listener; earlier finite
    -- init changes (3, 0) must NEVER be delivered to this post-load listener.
    SetReinforcementCount(1, -1)
    assert(GetReinforcementCount(1) == -1, "team 1 must be unlimited")

    OnTicketCountChange(function(team, count)
        assert(team ~= 1, "received a setup adjustment after changing to unlimited")
        SetTeamPoints(team, 3 + count)
    end)

    -- Invalid writes must be refused without latching outcome or breaking later valid writes.
    SetTeamPoints(99, 50)
    SetReinforcementCount(99, 5)
    MissionVictory(0)

    -- Trigger valid outcome on death via table dispatch:
    -- team 1 kills -> MissionVictory(1); team 2 kills -> MissionVictory({1, 2}) draw.
    local outcome_by_team = { [1] = 1, [2] = { 1, 2 } }
    OnCharacterDeath(function(_, killer)
        local side = killer and GetCharacterTeam(killer) or 0
        local result = outcome_by_team[side]
        if result ~= nil then
            MissionVictory(result)
        end
    end)
end
