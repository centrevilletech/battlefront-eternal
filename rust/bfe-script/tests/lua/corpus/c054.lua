-- Generated differential program 054 (generate.py, seed 503).
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
-- recursion
do
  local function fib(k) if k < 2 then return k end return fib(k - 1) + fib(k - 2) end
  print('fib', fib(5))
  local function down(k) if k == 0 then return 'bottom' end return down(k - 1) end
  print(down(5000))
  local function depth(k) if k == 0 then return 0 end return 1 + depth(k - 1) end
  print(depth(10))
  local function ack(m, k) if m == 0 then return k + 1 elseif k == 0 then return ack(m - 1, 1) end return ack(m - 1, ack(m, k - 1)) end
  print('ack', ack(2, 1))
end
-- closures
do
  local fs = {}
  for i = 1, 4 do fs[i] = function() return i * 5 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(3), counter(12)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
-- coerce
do
  print("5" + "0.25")
  print(3 .. "x")
  print(-"2")
  print("0x10" + 0)
  print(123456789 .. "")
  print("10" + 5)
  print("7" * "6")
  print(-0 .. "")
  print("1 2", tonumber("1 2"))
  print(".5e1", tonumber(".5e1"))
  print(" 12 ", tonumber(" 12 "))
  print("0x1p4", tonumber("0x1p4"))
  print(".", tonumber("."))
  print("-7.5", tonumber("-7.5"))
  print("3x", tonumber("3x"))
  print(27, tonumber("l04", 27))
  print(6, tonumber("055", 6))
  print(36, tonumber("k", 36))
  print(35, tonumber("a", 35))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
