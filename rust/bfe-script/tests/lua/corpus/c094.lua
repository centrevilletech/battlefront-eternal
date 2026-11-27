-- Generated differential program 094 (generate.py, seed 503).
-- coerce
do
  print(" 3e2 " * 1)
  print(-"2")
  print(123456789 .. "")
  print(1e15 .. "")
  print(1/0 .. "")
  print("7" * "6")
  print("5" + "0.25")
  print(-1/0 .. "")
  print("+3", tonumber("+3"))
  print(".", tonumber("."))
  print("5.", tonumber("5."))
  print("0x1F", tonumber("0x1F"))
  print("1e", tonumber("1e"))
  print("3x", tonumber("3x"))
  print("-inf", tonumber("-inf"))
  print(29, tonumber("63", 29))
  print(27, tonumber("5kjl", 27))
  print(16, tonumber("04 d", 16))
  print(29, tonumber("7d1", 29))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- closures
do
  local fs = {}
  for i = 1, 5 do fs[i] = function() return i * 1 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(4), counter(17)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
-- trig
do
  print('atan', math.atan(3))
  print('tan', math.tan(2))
  print('sin', math.sin(0.5))
  print('rad', math.rad(1))
  print('atan2', math.atan2(1, 2))
  print('pi', math.pi, math.pi * 2)
end
-- recursion
do
  local function fib(k) if k < 2 then return k end return fib(k - 1) + fib(k - 2) end
  print('fib', fib(8))
  local function down(k) if k == 0 then return 'bottom' end return down(k - 1) end
  print(down(5000))
  local function depth(k) if k == 0 then return 0 end return 1 + depth(k - 1) end
  print(depth(1000))
  local function ack(m, k) if m == 0 then return k + 1 elseif k == 0 then return ack(m - 1, 1) end return ack(m - 1, ack(m, k - 1)) end
  print('ack', ack(2, 2))
end
