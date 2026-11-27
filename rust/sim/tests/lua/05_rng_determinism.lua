-- Original test fixture for seeded script RNG determinism.

function ScriptPostLoad()
    OnCharacterDeath(function(victim, killer)
        if not killer then
            return
        end
        local roll = math.random(1, 100)
        AddTeamPoints(GetCharacterTeam(killer), roll)
    end)
end
