-- Generated differential program 079 (generate.py, seed 503).
-- strings
do
  local s = "Zef"
  print(string.sub(s, -1, 2), string.sub(s, -1), string.byte(s, 2))
  print(string.sub(s, -5, 1), string.sub(s, -5), string.byte(s, 1))
  print(string.sub(s, -9, -15), string.sub(s, -9), string.byte(s, -15))
  print(string.sub(s, -7, 3), string.sub(s, -7), string.byte(s, 3))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("ab", 5))
  print(string.char(60, 111, 118, 66, 116))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- recursion
do
  local function fib(k) if k < 2 then return k end return fib(k - 1) + fib(k - 2) end
  print('fib', fib(5))
  local function down(k) if k == 0 then return 'bottom' end return down(k - 1) end
  print(down(100))
  local function depth(k) if k == 0 then return 0 end return 1 + depth(k - 1) end
  print(depth(2500))
  local function ack(m, k) if m == 0 then return k + 1 elseif k == 0 then return ack(m - 1, 1) end return ack(m - 1, ack(m, k - 1)) end
  print('ack', ack(2, 4))
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
-- closures
do
  local fs = {}
  for i = 1, 3 do fs[i] = function() return i * 1 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(2), counter(13)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
