-- Error locations name the chunk. The test rewrites this chunk's source name
-- (long, multi-byte, `@`, `=` and literal forms) and checks the prefixes.
local ok, e = pcall(function() error("first") end)
print(e)
local ok2, e2 = pcall(function() local t = nil; return t.field end)
print(e2)
local ok3, e3 = pcall(function() return 1 + {} end)
print(e3)
local ok4, e4 = pcall(string.rep)
print(e4)
