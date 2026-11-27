function ScriptPostLoad()
    local scenario = ScriptCB_GetASSNumBots()
    if scenario == 1 then return end
    if scenario <= 4 then
        SetTeamPoints(1, 2)
        if scenario == 3 then SetTeamPoints(2, 4) end
        if scenario == 4 then SetTeamPoints(2, 2) end
    elseif scenario == 5 or scenario == 6 then
        if scenario == 5 then SetTeamPoints(2, 1) end
        local clock = CreateTimer("attendance")
        SetTimerValue(clock, 0.01)
        OnTimerElapse(function() error("missing sheet") end, clock)
        StartTimer(clock)
    elseif scenario == 7 then
        OnCharacterDeath(function() while true do end end)
    elseif scenario == 8 then
        OnTicketCountChange(function() error("missing register") end)
        SetReinforcementCount(2, 0)
    elseif scenario == 9 then
        OnCharacterDeath(function() MissionVictory(nil) end)
    elseif scenario == 10 then
        SetTeamPoints(1, 2)
        local clock = CreateTimer("adjourn")
        SetTimerValue(clock, 30)
        OnTimerElapse(function() MissionVictory(2) end, clock)
        StartTimer(clock)
    elseif scenario == 11 or scenario == 12 then
        SetTeamPoints(1, 2)
        SetTeamPoints(1, 1)
        if scenario == 11 then
            SetTeamPoints(2, 3)
        else
            local clock = CreateTimer("recount")
            SetTimerValue(clock, 40)
            OnTimerElapse(function() SetTeamPoints(2, 2) end, clock)
            StartTimer(clock)
        end
    end
end
