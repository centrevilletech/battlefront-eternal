-- Deliver a crate on the bell. Closing the bell cancels its other recipient.
local bell

function ScriptInit()
    SetReinforcementCount(2, 9)
    bell = CreateTimer("delivery")
    SetTimerValue(bell, 4)
    StartTimer(bell)
end

function ScriptPostLoad()
    ShowTimer(bell)
    OnTicketCountChange(function(account, balance)
        SetTeamPoints(account, balance * 3)
    end)
    OnTimerElapse(function(clock)
        StopTimer(clock)
        AddReinforcements(2, 7)
    end, bell)
    OnTimerElapse(function()
        error("closed bell rang twice")
    end, bell)
end
