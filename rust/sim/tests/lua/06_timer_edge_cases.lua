-- Original test fixture for timer retention across callback caps and
-- stop/reset/destroy invalidation of pending expiries.

function ScriptPostLoad()
    -- Scenario A: one timer with two listeners; callback cap 1 interrupts between them.
    local dual = CreateTimer("dual_clock")
    SetTimerValue(dual, 0.25)
    StartTimer(dual)
    OnTimerElapse(function()
        AddTeamPoints(1, 2)
    end, dual)
    OnTimerElapse(function()
        AddTeamPoints(1, 5)
    end, dual)

    -- Scenario B: three timers that a death listener stops/resets/destroys on their due tick.
    local t_stop = CreateTimer("stop_clock")
    SetTimerValue(t_stop, 0.5)
    StartTimer(t_stop)
    OnTimerElapse(function()
        AddTeamPoints(2, 10)
    end, t_stop)

    local t_reset = CreateTimer("reset_clock")
    SetTimerValue(t_reset, 0.5)
    StartTimer(t_reset)
    OnTimerElapse(function()
        AddTeamPoints(2, 20)
    end, t_reset)

    local t_destroy = CreateTimer("destroy_clock")
    SetTimerValue(t_destroy, 0.5)
    StartTimer(t_destroy)
    OnTimerElapse(function()
        AddTeamPoints(2, 40)
    end, t_destroy)

    OnCharacterDeath(function()
        StopTimer(t_stop)
        SetTimerValue(t_reset, 5.0)
        DestroyTimer(t_destroy)
    end)
end
