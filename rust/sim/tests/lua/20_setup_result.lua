local scenario
function ScriptInit()
    scenario = ScriptCB_GetASSNumBots()
    if scenario == 1 then error("register unavailable") end
end
function ScriptPostLoad()
    SetTeamPoints(1, 3)
    MissionVictory(1)
    if scenario == 2 then error("register closed") end
end
