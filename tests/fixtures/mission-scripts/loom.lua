local function spool(thread)
    return ScriptCB_DoFile(thread)
end

spool("WeFt")
pcall(spool, "AbsentThread")
spool("WEFT")
spool("Spare")
