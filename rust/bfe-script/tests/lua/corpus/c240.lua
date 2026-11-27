-- Fixed differential program: environment levels (generate.py).
local env = _G
for _, level in ipairs({0, 0.5, -0.5, "0.75", "0", -0, 0.25, "0x0", " 0 "}) do
  local ok, err = pcall(setfenv, level, env)
  print(level, ok, err)
end
print(getfenv(0) == _G, getfenv(0.5) == _G, getfenv("0.75") == _G, getfenv(-0) == _G)
print(pcall(getfenv, -1))
local ok, found = pcall(getfenv, -0.5)
print(ok, found == _G)
local f = function() return 1 end
print(pcall(setfenv, f, env) == true, getfenv(f) == env)
print(pcall(setfenv, print, env))
print(pcall(setfenv, 1.5, env))
