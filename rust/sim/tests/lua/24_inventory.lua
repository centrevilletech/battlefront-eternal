-- Count every crate, then file its paperwork. A bell closes the shift.
function ScriptPostLoad()
    local bell = CreateTimer("shift")
    SetTimerValue(bell, 0.05)
    StartTimer(bell)
    OnTicketCountChange(function()
        AddTeamPoints(1, 1)
        FileInventory()
        AddTeamPoints(1, 2)
    end)
    OnTimerElapse(function() AddTeamPoints(2, 10) end, bell)
    OnCharacterDeath(function() AddTeamPoints(2, 1) end)
end
