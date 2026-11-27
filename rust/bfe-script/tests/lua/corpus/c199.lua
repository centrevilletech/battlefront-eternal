-- Generated differential program 199 (generate.py, seed 503).
-- coerce
do
  print("7" * "6")
  print("  12  " + 0)
  print(16777217 .. "")
  print("0x10" + 0)
  print(-"2")
  print("1e3" / 4)
  print(".5" .. 1)
  print(1e15 .. "")
  print("0x", tonumber("0x"))
  print("+3", tonumber("+3"))
  print("12", tonumber("12"))
  print("1e", tonumber("1e"))
  print("inf", tonumber("inf"))
  print(".5e1", tonumber(".5e1"))
  print("1 2", tonumber("1 2"))
  print(4, tonumber("222", 4))
  print(20, tonumber("299", 20))
  print(29, tonumber("pp3ga9", 29))
  print(12, tonumber("9 5", 12))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- errors
do
  print(pcall(error, 'plain'))
  print(pcall(error, 's'))
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
  print(pcall(string.format, "[%x]", 123456))
  print(pcall(string.format, "[% 19g]", 938.96930))
  print(pcall(string.format, "[%#x]", 0))
  print(pcall(string.format, "[%18x]", -1))
  print(pcall(string.format, "[%5.2s]", "hello"))
  print(pcall(string.format, "[%#+f]", 940.47013))
  print(pcall(string.format, "[%q]", "back\\slash"))
  print(pcall(string.format, "[%o]", 0))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
