-- Generated differential program 221 (generate.py, seed 503).
-- loops
do
  do local n, last = 0, nil for i = 1, 2.6, 1 do n = n + 1 last = i if n > 50 then break end end print('steps', n, last) end
  do local s = 0 for i = 1, 29 do if math.mod(i, 3) == 0 then s = s + i end end print('sum', s) end
  do local k = 0 repeat k = k + 2 until k >= 11 print('repeat', k) end
  do local x = 0.1 local m = 0 while x < 1 do x = x + 0.1 m = m + 1 end print('float loop', m, x) end
  do local t = {} for i = 1, 5 do t[i] = i * i end local s = '' for i, v in ipairs(t) do s = s .. v .. ';' end print(s) end
end
-- globals
do
  gx = 58
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
  local s = "ZYZX10"
  print(string.sub(s, 2, 14), string.sub(s, 2), string.byte(s, 14))
  print(string.sub(s, 13, -11), string.sub(s, 13), string.byte(s, -11))
  print(string.sub(s, -12, 9), string.sub(s, -12), string.byte(s, 9))
  print(string.sub(s, 12, 7), string.sub(s, 12), string.byte(s, 7))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("-", 4))
  print(string.char())
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
