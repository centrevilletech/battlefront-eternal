-- Original roster helper loaded via ScriptCB_DoFile by 01_arena_match.lua.
-- Written independently for Battlefront Eternal tests (no game data).

function ConfigureArenaTeams()
    assert(ScriptCB_IsMissionSetupSaved() == false, "no saved setup expected")
    assert(ScriptCB_GetPlatform() == "PC", "expected PC platform")

    local rosters = {
        { id = 1, label = "vanguard", icons = { "badge_v", "ticket_v", "banner_v" }, count = 12, classes = { "rep_soldier", "rep_runner" } },
        { id = 2, label = "sentinel", icons = { "badge_s", "ticket_s", "banner_s" }, count = 12, classes = { "cis_soldier" } },
    }
    for _, side in ipairs(rosters) do
        SetTeamName(side.id, side.label)
        SetTeamIcon(side.id, side.icons[1], side.icons[2], side.icons[3])
        SetUnitCount(side.id, side.count)
        SetReinforcementCount(side.id, -1)
        for slot, cls in ipairs(side.classes) do
            AddUnitClass(side.id, cls, slot, slot + 2)
        end
    end
end
