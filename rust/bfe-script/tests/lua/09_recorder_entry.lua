-- Synthetic mission script exercising ScriptCB_DoFile, ScriptPreInit, ScriptInit,
-- undefined global guards, nested tables, function markers, and skipping ScriptPostLoad.
ScriptCB_DoFile("09_recorder_helper")
ScriptCB_DoFile("09_recorder_helper") -- second load hits include guard

function ScriptPreInit()
    HostSetExtents(2000)
end

function ScriptInit()
    if OptionalUndefinedHook then
        OptionalUndefinedHook("should_not_run")
    end
    HostLoadArchive("world.lvl", "mode_a")
    HelperSetup({
        team = 1,
        items = {
            { "class_a", 4 },
            { "class_b", 2 },
        },
    })
    local nested = {
        flag = true,
        empty = {},
        cb = function() return 1 end,
    }
    nested.self_ref = nested
    HostRegister(nested, nil, false, 1.5, function() end)
end

function ScriptPostLoad()
    HostShouldNotRun("post_load_was_called")
end
