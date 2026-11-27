-- Generated differential program 229 (generate.py, seed 503).
-- recursion
do
  local function fib(k) if k < 2 then return k end return fib(k - 1) + fib(k - 2) end
  print('fib', fib(16))
  local function down(k) if k == 0 then return 'bottom' end return down(k - 1) end
  print(down(20000))
  local function depth(k) if k == 0 then return 0 end return 1 + depth(k - 1) end
  print(depth(300))
  local function ack(m, k) if m == 0 then return k + 1 elseif k == 0 then return ack(m - 1, 1) end return ack(m - 1, ack(m, k - 1)) end
  print('ack', ack(2, 2))
end
-- coerce
do
  print(16777217 .. "")
  print(1e15 .. "")
  print("7" * "6")
  print(" 3e2 " * 1)
  print("  12  " + 0)
  print("10" + 5)
  print(".5" .. 1)
  print(3 .. "x")
  print("3x", tonumber("3x"))
  print("0x1F", tonumber("0x1F"))
  print("0x1p4", tonumber("0x1p4"))
  print(".", tonumber("."))
  print("inf", tonumber("inf"))
  print("5.", tonumber("5."))
  print(".5e1", tonumber(".5e1"))
  print(24, tonumber("h4j", 24))
  print(21, tonumber("ia  8b", 21))
  print(16, tonumber("911a0e", 16))
  print(7, tonumber("361135", 7))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- closures
do
  local fs = {}
  for i = 1, 5 do fs[i] = function() return i * 8 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(5), counter(11)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
-- strings
do
  local s = "YYYbZ9 1"
  print(string.sub(s, -5, -6), string.sub(s, -5), string.byte(s, -6))
  print(string.sub(s, -4, 0), string.sub(s, -4), string.byte(s, 0))
  print(string.sub(s, 11, -6), string.sub(s, 11), string.byte(s, -6))
  print(string.sub(s, 11, 7), string.sub(s, 11), string.byte(s, 7))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("", 2))
  print(string.char())
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
