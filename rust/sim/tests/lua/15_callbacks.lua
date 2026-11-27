-- A tally kept by independent subscriptions.
local mode
function ScriptInit()
    mode = ScriptCB_GetASSNumBots()
end

function ScriptPostLoad()
    local visits = 0
    if mode == 1 then
        OnCharacterDeath(function()
            visits = visits + 1
            AddTeamPoints(1, 1)
            if math.mod(visits, 2) == 0 then error("parcel " .. visits) end
            error("receipt " .. visits)
        end)
        OnCharacterDeath(function()
            AddTeamPoints({}, 1)
        end)
        OnCharacterDeath(function()
            AddTeamPoints(2, 1)
        end)
    elseif mode == 2 then
        OnCharacterDeath(function()
            visits = visits + 1
            AddTeamPoints(1, 1)
            if visits ~= 3 then
                for read = 1, 36 do GetTeamPoints(1) end
            end
        end)
        OnCharacterDeath(function() AddTeamPoints(2, 1) end)
    elseif mode == 3 then
        SetTeamPoints(1, 2)
        local bell = CreateTimer("counted")
        SetTimerValue(bell, 0.05)
        OnTimerElapse(function() local receipt = nil; receipt() end, bell)
        OnTimerElapse(function() MissionVictory(1) end, bell)
        StartTimer(bell)
    elseif mode == 8 then
        OnCharacterDeath(function()
            visits = visits + 1
            local missing = nil
            if math.mod(visits, 2) == 1 then missing() end
            missing()
        end)
        OnCharacterDeath(function() local missing = nil; missing() end)
        OnCharacterDeath(function() local missing = nil; missing() end)
    elseif mode >= 4 then
        OnCharacterDeath(function()
            if mode == 4 then
                AddTeamPoints(2, 1)
            elseif mode == 5 then
                SetTeamPoints(1, 2)
                SetTeamPoints(2, 5)
            elseif mode == 6 then
                SetTeamPoints(2, 2)
                SetTeamPoints(1, 2)
            elseif mode == 7 then
                MissionVictory(2)
                SetTeamPoints(1, 4)
            end
            string.rep("folio", 500000)
        end)
    end
end
