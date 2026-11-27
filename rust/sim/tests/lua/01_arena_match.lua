ScriptCB_DoFile("01_arena_roster")

function ScriptPreInit()
    SetWorldExtents(420)
end

function ScriptInit()
    ConfigureArenaTeams()
    SetSpawnDelay(4, 0.5)
end

-- A rescue contest. A tagged runner earns a rescue stamp for their own side.
-- The score announcement decides who has filled a card.
function ScriptPostLoad()
    local card = ScriptCB_GetAssaultScoreLimit()
    local function stamp(runner, tagger)
        if tagger then
            AddTeamPoints(GetCharacterTeam(runner), 1)
        end
    end
    OnCharacterDeath(stamp)
    OnTeamPointsChange(function(owner, stamps)
        if stamps == card then
            MissionVictory(owner)
        end
    end)
end
