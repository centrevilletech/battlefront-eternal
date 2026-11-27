-- Original test fixture for unbound host function handling, runtime error
-- isolation, and per-callback instruction budget enforcement.

function ScriptPostLoad()
    -- Unbound global function call during PostLoad (fails in strict mode,
    -- logged no-op in permissive mode).
    UnboundHostFunctionAlpha("arg1", 42)

    -- This listener raises an error on every death.
    OnCharacterDeath(function(victim, killer)
        error("deliberate callback failure")
    end)

    -- This independent listener remains registered when its predecessor faults.
    OnCharacterDeath(function(victim, killer)
        if killer then
            AddTeamPoints(GetCharacterTeam(killer), 1)
        end
    end)

    -- Timer callback that spins forever to test instruction budget enforcement.
    local t = CreateTimer("spin_timer")
    SetTimerValue(t, 0.1)
    StartTimer(t)
    OnTimerElapse(function()
        while true do
        end
    end, t)
end
