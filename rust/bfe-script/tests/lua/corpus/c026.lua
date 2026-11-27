-- Generated differential program 026 (generate.py, seed 503).
-- coerce
do
  print(2^24 + 1)
  print(1e100 .. "")
  print(".5" .. 1)
  print(1e15 .. "")
  print(" 3e2 " * 1)
  print("5" + "0.25")
  print(0.1 .. "")
  print(16777217 .. "")
  print("0x1p4", tonumber("0x1p4"))
  print("1e", tonumber("1e"))
  print("1e400", tonumber("1e400"))
  print("-inf", tonumber("-inf"))
  print(" 12 ", tonumber(" 12 "))
  print("3x", tonumber("3x"))
  print("inf", tonumber("inf"))
  print(9, tonumber("2", 9))
  print(5, tonumber("2 33 1", 5))
  print(15, tonumber("c6", 15))
  print(30, tonumber("1 19hl", 30))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- format
do
  print(pcall(string.format, "[%11d]", 2147483647))
  print(pcall(string.format, "[%d]", 7))
  print(pcall(string.format, "[%9g]", 0.989022931))
  print(pcall(string.format, "[%16.10s]", "hello"))
  print(pcall(string.format, "[%-#14.2g]", 11e3))
  print(pcall(string.format, "[%19.10e]", 16777216))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
-- closures
do
  local fs = {}
  for i = 1, 3 do fs[i] = function() return i * 8 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(8), counter(19)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
