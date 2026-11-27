local catalog = AddAIGoal(0, "Defend", 0.5, "bookcase")
function ScriptPreInit()
    AddAIGoal(2, "Deathmatch", 1)
end
function ScriptInit()
    DeleteAIGoal(ScriptCB_GetASSNumBots())
    assert(catalog < AddAIGoal(1, "Conquest", 2))
end
