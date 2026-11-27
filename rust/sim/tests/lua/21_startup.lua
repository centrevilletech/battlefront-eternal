ScriptCB_DoFile("previous_setup")
local initialize = ScriptInit
local finish = ScriptPostLoad

function ScriptInit()
    if ScriptCB_GetASSNumBots() == 1 then error("no attendance list") end
    initialize()
end

function ScriptPostLoad()
    finish()
    local count = ScriptCB_GetASSNumBots()
    if count == 2 or count == 16 then
        SetTeamPoints(1, 3)
        MissionVictory(1)
        error("attendance list closed")
    end
end
