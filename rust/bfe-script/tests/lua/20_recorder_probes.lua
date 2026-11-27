-- Synthetic mission script: existence tests, calls through tables and `_G`,
-- globals assigned through `_G`, cycle distances and shared subtables.
if OptionalApi then
    OptionalApi(1)
end
local t = { fn = FieldStored }
if t.fn then
    t.fn(3)
end
local h = rawget(_G, "RawProbe")
if h then
    h(4)
end
local g = _G.ViaG
g(5)
if _G.TestedViaG then
    Record("tested_via_g")
end
_G.AssignedViaG = function() end
AssignedViaG(6)
rawset(_G, "AssignedRaw", function() end)
AssignedRaw(7)
local cyc = {}
cyc.inner = { back = cyc }
cyc.inner.self = cyc.inner
Record(cyc)
local s = { 1, 2 }
Record({ s, s })
