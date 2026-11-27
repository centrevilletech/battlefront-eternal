-- Original test fixture for shared per-tick VM/native budget, host cap
-- enforcement, and deterministic authority-visible abort on unrecoverable fault.

function ScriptPostLoad()
    -- Register 8 death listeners each running a small loop so their COMBINED
    -- instructions exceed a tight shared tick_instruction_budget while any single
    -- listener stays under it.
    for idx = 1, 8 do
        OnCharacterDeath(function()
            local sum = 0
            for j = 1, 40 do
                sum = sum + j
            end
            AddTeamPoints(1, 1)
        end)
    end

    -- Attempt to spam AI goals beyond the available goal slots.
    for g = 1, 150 do
        AddAIGoal(1, "Deathmatch", 10)
    end
end
