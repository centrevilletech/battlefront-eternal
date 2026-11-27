-- Generated differential program 200 (generate.py, seed 503).
-- strings
do
  local s = "f ccc"
  print(string.sub(s, -8, 15), string.sub(s, -8), string.byte(s, 15))
  print(string.sub(s, -4, 0), string.sub(s, -4), string.byte(s, 0))
  print(string.sub(s, 12, 15), string.sub(s, 12), string.byte(s, 15))
  print(string.sub(s, -10, -10), string.sub(s, -10), string.byte(s, -10))
  print(string.sub(s, 2, 3), string.sub(s, 2), string.byte(s, 3))
  print(string.sub(s, 10, -10), string.sub(s, 10), string.byte(s, -10))
  print(string.sub(s, -12, -15), string.sub(s, -12), string.byte(s, -15))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("", 1))
  print(string.char(38, 116, 76, 62))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- globals
do
  gx = 49
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
end
-- loops
do
  do local n, last = 0, nil for i = 10, 2.6, -1 do n = n + 1 last = i if n > 50 then break end end print('steps', n, last) end
  do local s = 0 for i = 1, 25 do if math.mod(i, 3) == 0 then s = s + i end end print('sum', s) end
  do local k = 0 repeat k = k + 2 until k >= 6 print('repeat', k) end
  do local x = 0.1 local m = 0 while x < 1 do x = x + 0.1 m = m + 1 end print('float loop', m, x) end
  do local t = {} for i = 1, 5 do t[i] = i * i end local s = '' for i, v in ipairs(t) do s = s .. v .. ';' end print(s) end
end
-- varargs
do
  local function count(...) return arg.n end
  local function pass(...) return unpack(arg) end
  print(count(), count(nil), count(1, nil), count(nil, nil, 3))
  print(pass(1, 2, 3))
  print((pass(1, 2, 3)))
  local function tail(n, ...) if n == 0 then return arg.n end return tail(n - 1, n, unpack(arg)) end
  print(tail(7))
  local a, b, c = pass(1, 7, 4)
  print(a, b, c)
  local t = {pass(1, 2), pass(3, 4)} print(table.getn(t), t[1], t[2], t[3])
end
