-- Two crews keep watch. Nothing is won and nothing ends: players come, fall
-- and come back with the next wave.
local crews = {
    { side = 1, name = "blue", class = "rep_winder" },
    { side = 2, name = "gold", class = "cis_winder" },
}

function ScriptInit()
    for _, crew in ipairs(crews) do
        SetTeamName(crew.side, crew.name)
        SetUnitCount(crew.side, 4)
        SetReinforcementCount(crew.side, -1)
        AddUnitClass(crew.side, crew.class, 1, 32)
    end
    SetSpawnDelay(5, 0.5)
end

function ScriptPostLoad()
    AddMissionObjective(0, "vigil.watch")
    ActivateObjective("vigil.watch")
end
