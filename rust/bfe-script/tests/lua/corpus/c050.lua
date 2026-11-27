-- Generated differential program 050 (generate.py, seed 503).
-- errors
do
  print(pcall(error, 'plain'))
  print(pcall(error, 42))
  print(pcall(error))
  local function lvl1() error('one', 1) end local function lvl2() error('two', 2) end
  print(pcall(lvl1)) print(pcall(lvl2))
  print(pcall(function() local t = nil return t.x end))
  print(pcall(function() return undefined_global_fn() end))
  print(pcall(function() local t = {} return t.a.b end))
  print(pcall(function() return 1 < 'x' end))
  print(pcall(function() return {} .. 'x' end))
  print(pcall(function() return -{} end))
  print(pcall(function() local s = 'x' return s + 1 end))
  print(pcall(function() local t = setmetatable({}, {__index = function(t, k) error('deep ' .. k) end}) return t.key end))
  print(xpcall(function() error('boom') end, function(m) return 'handled: ' .. m end))
  print(pcall(error, setmetatable({}, {__tostring = function() return 'obj' end})) == false)
  print(pcall(assert, false, 'assert msg'))
  print(pcall(assert, nil))
  print(pcall(assert, 1, 2))
end
-- compare
do
  print(-555 < 3, -555 <= 3, -555 == 3)
  print(-24.432 < 0.5622232954, -24.432 <= 0.5622232954, -24.432 == 0.5622232954)
  print(16777215 < 0.3, 16777215 <= 0.3, 16777215 == 0.3)
  print(904.27707 < 597.96827, 904.27707 <= 597.96827, 904.27707 == 597.96827)
  print("0a" < "BA9", "0a" <= "BA9", "0a" == "BA9")
  print("b9" < "0c0", "b9" <= "0c0", "b9" == "0c0")
  print("a_c_" < "c", "a_c_" <= "c", "a_c_" == "c")
end
-- globals
do
  gx = 84
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
end
-- format
do
  print(pcall(string.format, "[%#13g]", -924))
  print(pcall(string.format, "[%#18.1G]", 16777215))
  print(pcall(string.format, "[%4c]", 97))
  print(pcall(string.format, "[%q]", "line1\nline2"))
  print(pcall(string.format, "[%0g]", -601))
  print(pcall(string.format, "[% 12X]", 123456))
  print(pcall(string.format, "[%13c]", 48))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
