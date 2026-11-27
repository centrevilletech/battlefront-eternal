local function tally(limit)
    local count = 0
    while count < limit do count = count + 1 end
end

function ScriptPostLoad()
    OnCharacterDeath(function() tally(14750) end)
    OnCharacterDeath(function()
        tally(400)
        AddTeamPoints(1, 1)
    end)
end
