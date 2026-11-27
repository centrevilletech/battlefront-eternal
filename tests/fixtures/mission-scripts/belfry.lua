-- Two crews wind a belfry. Each time the winch stops, both tallies grow and
-- the winch is wound again. A full tally rings the bell for its crew.
local crews = {
    { side = 1, name = "blue", class = "rep_winder", badge = "badge_blue" },
    { side = 2, name = "gold", class = "cis_winder", badge = "badge_gold" },
}
local turns = { 40, 10 }

function ScriptPreInit()
    SetWorldExtents(900)
end

function ScriptInit()
    SetMaxFlyHeight(90)
    for _, crew in ipairs(crews) do
        SetTeamName(crew.side, crew.name)
        SetTeamIcon(crew.side, crew.badge, crew.badge .. "_ticket", crew.badge .. "_flag")
        SetUnitCount(crew.side, 4)
        SetReinforcementCount(crew.side, 50 * crew.side)
        AddUnitClass(crew.side, crew.class, 1, 4)
    end
    SetSpawnDelay(1, 0.25)
end

function ScriptPostLoad()
    local full = ScriptCB_GetAssaultScoreLimit()
    AddMissionObjective(1, "belfry.wind", "belfry.wind.more")
    AddMissionObjective(2, "belfry.guard")
    AddMissionObjective(0, "belfry.listen")
    ActivateObjective("belfry.wind")
    ActivateObjective("belfry.listen")
    ShowTeamPoints(1)
    ShowTeamPoints(2)
    AddMissionHint("belfry.hint")

    local winch = CreateTimer("winch")
    SetTimerValue(winch, 1.5)
    OnTimerElapse(function(timer)
        for side, turn in ipairs(turns) do
            AddTeamPoints(side, turn)
        end
        if GetTeamPoints(1) >= full then
            CompleteObjective("belfry.wind")
            MissionVictory(1)
        else
            ShowMessageText("belfry.turn", 1)
            ShowMessageText("belfry.everyone")
            BroadcastVoiceOver("belfry_creak", 2)
            SetTimerValue(timer, 1.5)
            StartTimer(timer)
        end
    end, winch)
    ShowTimer(winch)
    StartTimer(winch)
end
