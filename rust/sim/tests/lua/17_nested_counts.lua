local function count_below()
    return 1 + count_below()
end

function ScriptPostLoad()
    OnCharacterDeath(function() count_below() end)
    OnCharacterDeath(function() AddTeamPoints(1, 1) end)
end
