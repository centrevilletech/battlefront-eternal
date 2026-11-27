-- A maintenance beacon announces each change in its own service state.
local function announce(beacon)
    ShowMessageText(GetEntityName(beacon), 1)
    return beacon, "acknowledged", 17
end
function ScriptPostLoad()
    OnObjectDamageName(announce, "beacon")
    OnObjectRespawnName(announce, "beacon")
    OnObjectKillName(announce, "beacon")
end
