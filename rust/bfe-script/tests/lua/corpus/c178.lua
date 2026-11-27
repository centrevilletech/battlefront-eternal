-- Generated differential program 178 (generate.py, seed 503).
-- coerce
do
  print(-0 .. "")
  print(" 3e2 " * 1)
  print("0x10" + 0)
  print(1e100 .. "")
  print(".5" .. 1)
  print(-1/0 .. "")
  print(1e15 .. "")
  print("1e3" / 4)
  print("1e400", tonumber("1e400"))
  print(" 12 ", tonumber(" 12 "))
  print(".5e1", tonumber(".5e1"))
  print("+3", tonumber("+3"))
  print("0x1F", tonumber("0x1F"))
  print("0x", tonumber("0x"))
  print("12", tonumber("12"))
  print(13, tonumber("b350", 13))
  print(32, tonumber("c", 32))
  print(20, tonumber("415h", 20))
  print(24, tonumber("3h", 24))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- globals
do
  gx = 51
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
end
-- errors
do
  print(pcall(error, 'plain'))
  print(pcall(error, true))
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
-- format
do
  print(pcall(string.format, "[%+ 8f]", 7))
  print(pcall(string.format, "[%q]", "plain"))
  print(pcall(string.format, "[%.1g]", 2^27))
  print(pcall(string.format, "[%d]", 123456))
  print(pcall(string.format, "[%18X]", -1))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
