-- Environment levels: a level that converts to 0 selects the running native
-- (`setfenv` itself) unless the argument is exactly 0; only then are the
-- thread's globals replaced.
local env = _G
for _, level in ipairs({0, 0.5, -0.5, "0.75", "0", -0, 0.25}) do
  local ok, err = pcall(setfenv, level, env)
  print(level, ok, err)
end
print(getfenv(0) == _G, getfenv(0.5) == _G, getfenv("0.75") == _G)
print(pcall(getfenv, -1))
local f = function() return 1 end
print(pcall(setfenv, f, env) == true, getfenv(f) == env)
print(pcall(setfenv, print, env))
