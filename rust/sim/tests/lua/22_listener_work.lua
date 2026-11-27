local function count_to(limit)
    local count = 0
    while count < limit do count = count + 1 end
end

function ScriptPostLoad()
    local case = ScriptCB_GetASSNumBots()
    local cost = 8000
    if case == 2 then cost = 9000 end
    if case == 3 then cost = 17000 end
    OnCharacterDeath(function() count_to(cost) end)
    if case == 2 then
        OnCharacterDeath(function() count_to(3250) end)
        OnCharacterDeath(function() count_to(3250) end)
    end
    OnCharacterDeath(function() AddTeamPoints(1, 1) end)
end
