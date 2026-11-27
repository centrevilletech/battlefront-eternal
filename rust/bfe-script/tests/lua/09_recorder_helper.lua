-- Synthetic helper script loaded via ScriptCB_DoFile in recorder tests.
if __SYNTHETIC_HELPER_LOADED__ then
    return
end
__SYNTHETIC_HELPER_LOADED__ = 1

function HelperSetup(cfg)
    for _, item in ipairs(cfg.items) do
        HostAddItem(cfg.team, item[1], item[2])
    end
end
