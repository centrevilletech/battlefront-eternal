-- A belfry that cannot start: its crews are declared, and then the winch asks
-- for a part that was never made.
function ScriptInit()
    for side, class in ipairs({ "rep_winder", "cis_winder" }) do
        SetUnitCount(side, 2)
        AddUnitClass(side, class, 1, 2)
    end
end

function ScriptPostLoad()
    local winch = nil
    winch.turn()
end
