-- Each choice exercises one limit in a response to a character loss.
local choice
function ScriptInit()
    choice = ScriptCB_GetASSNumBots()
end
local function descend(n)
    return 1 + descend(n + 1)
end
function ScriptPostLoad()
    OnCharacterDeath(function()
        if choice == 1 then
            local n = 0
            while true do n = n + 1 end
        elseif choice == 2 then
            descend(1)
        elseif choice == 3 then
            string.rep("paper", 500000)
        elseif choice == 4 then
            AddTeamPoints({}, 2)
        elseif choice == 5 then
            AddAIGoal(1)
        elseif choice == 6 then
            SetTeamName(2, string.rep("k", 4100))
        elseif choice == 7 then
            error(string.rep("receipt", 512))
        else
            AddTeamPoints(2, choice)
        end
    end)
end
