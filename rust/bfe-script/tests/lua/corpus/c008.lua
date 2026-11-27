-- Generated differential program 008 (generate.py, seed 503).
-- globals
do
  gx = 79
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
end
-- strings
do
  local s = "Y9eb edcY"
  print(string.sub(s, 13, -8), string.sub(s, 13), string.byte(s, -8))
  print(string.sub(s, 6, -6), string.sub(s, 6), string.byte(s, -6))
  print(string.sub(s, 10, -7), string.sub(s, 10), string.byte(s, -7))
  print(string.sub(s, 2, 11), string.sub(s, 2), string.byte(s, 11))
  print(string.sub(s, 8, 15), string.sub(s, 8), string.byte(s, 15))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("", 0))
  print(string.char())
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- format
do
  print(pcall(string.format, "[%i]", 0))
  print(pcall(string.format, "[%#X]", 2147483647))
  print(pcall(string.format, "[%-7u]", 123456))
  print(pcall(string.format, "[%g]", -28.255))
  print(pcall(string.format, "[%q]", "nul\0byte"))
  print(pcall(string.format, "[%14X]", 123456))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
-- coerce
do
  print("5" + "0.25")
  print(-"2")
  print("1e3" / 4)
  print(3 .. "x")
  print("  12  " + 0)
  print("10" + 5)
  print(".5" .. 1)
  print(" 3e2 " * 1)
  print("0x", tonumber("0x"))
  print("  0x10  ", tonumber("  0x10  "))
  print(".", tonumber("."))
  print("1e400", tonumber("1e400"))
  print("0x1F", tonumber("0x1F"))
  print("-7.5", tonumber("-7.5"))
  print("3x", tonumber("3x"))
  print(3, tonumber("2", 3))
  print(17, tonumber("04", 17))
  print(13, tonumber("4", 13))
  print(14, tonumber("4", 14))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
