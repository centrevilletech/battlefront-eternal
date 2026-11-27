ScriptCB_DoFile("previous_setup")
local subscribe = OnObjectDamageName
local function count_sample()
    local n = 0
    while n < 9000 do n = n + 1 end
end

function OnObjectDamageName(callback, name)
    local original = subscribe(callback, name)
    subscribe(count_sample, name)
    return original
end
