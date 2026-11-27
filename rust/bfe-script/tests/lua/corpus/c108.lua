-- Generated differential program 108 (generate.py, seed 503).
-- strings
do
  local s = "b  9dZZeZ"
  print(string.sub(s, 8, -12), string.sub(s, 8), string.byte(s, -12))
  print(string.sub(s, -4, 10), string.sub(s, -4), string.byte(s, 10))
  print(string.sub(s, -4, 11), string.sub(s, -4), string.byte(s, 11))
  print(string.sub(s, 3, -6), string.sub(s, 3), string.byte(s, -6))
  print(string.sub(s, 15, -12), string.sub(s, 15), string.byte(s, -12))
  print(string.sub(s, 4, -4), string.sub(s, 4), string.byte(s, -4))
  print(string.sub(s, -11, 2), string.sub(s, -11), string.byte(s, 2))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("ab", 5))
  print(string.char(83, 77, 114, 73, 86))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- globals
do
  gx = 45
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
end
-- coerce
do
  print(1e100 .. "")
  print("1e3" / 4)
  print(0.1 .. "")
  print(10 .. 20)
  print(-"2")
  print(16777217 .. "")
  print(123456789 .. "")
  print(-0 .. "")
  print("1e400", tonumber("1e400"))
  print("0x1F", tonumber("0x1F"))
  print("-inf", tonumber("-inf"))
  print("  0x10  ", tonumber("  0x10  "))
  print("1 2", tonumber("1 2"))
  print("3x", tonumber("3x"))
  print("1e", tonumber("1e"))
  print(19, tonumber("4f i0e", 19))
  print(28, tonumber("3m138", 28))
  print(18, tonumber("7", 18))
  print(14, tonumber("c", 14))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- loops
do
  do local n, last = 0, nil for i = 1, 2.6, -1 do n = n + 1 last = i if n > 50 then break end end print('steps', n, last) end
  do local s = 0 for i = 1, 31 do if math.mod(i, 3) == 0 then s = s + i end end print('sum', s) end
  do local k = 0 repeat k = k + 2 until k >= 4 print('repeat', k) end
  do local x = 0.1 local m = 0 while x < 1 do x = x + 0.1 m = m + 1 end print('float loop', m, x) end
  do local t = {} for i = 1, 5 do t[i] = i * i end local s = '' for i, v in ipairs(t) do s = s .. v .. ';' end print(s) end
end
