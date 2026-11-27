-- A clerk files the paperwork for each shift. A porter writes a receipt with
-- sixteen lines for every arrival.
function ScriptPostLoad()
    OnTicketCountChange(function()
        AddTeamPoints(1, 1)
        FileInventory()
    end)
    OnCharacterDeath(function()
        local receipt = { 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1 }
        AddTeamPoints(2, receipt[16])
    end)
end
