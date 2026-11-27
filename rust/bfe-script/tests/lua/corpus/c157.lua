-- Generated differential program 157 (generate.py, seed 503).
-- format
do
  print(pcall(string.format, "[%0d]", 7))
  print(pcall(string.format, "[%11.11G]", -542))
  print(pcall(string.format, "[%E]", 2147483647))
  print(pcall(string.format, "[%5.2s]", "xxxxxxxxxxxx"))
  print(pcall(string.format, "[%17o]", 123456))
  print(pcall(string.format, "[%e]", -48))
  print(pcall(string.format, "[%16.5g]", 0.1))
  print(pcall(string.format, "[%0o]", -98765))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
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
-- coerce
do
  print("0x10" + 0)
  print("10" + 5)
  print(-"2")
  print(16777217 .. "")
  print(" 3e2 " * 1)
  print(3 .. "x")
  print(0.1 .. "")
  print("  12  " + 0)
  print("0x1p4", tonumber("0x1p4"))
  print("1e", tonumber("1e"))
  print("1e400", tonumber("1e400"))
  print("inf", tonumber("inf"))
  print("-7.5", tonumber("-7.5"))
  print(".5e1", tonumber(".5e1"))
  print("0x1F", tonumber("0x1F"))
  print(24, tonumber("e4i3a3", 24))
  print(3, tonumber("012221", 3))
  print(4, tonumber("33322", 4))
  print(17, tonumber("2", 17))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
