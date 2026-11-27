-- Generated differential program 093 (generate.py, seed 503).
-- closures
do
  local fs = {}
  for i = 1, 2 do fs[i] = function() return i * 5 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(9), counter(11)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
-- coerce
do
  print(1/0 .. "")
  print(123456789 .. "")
  print(10 .. 20)
  print(2^24 + 1)
  print(1e15 .. "")
  print(1e100 .. "")
  print(-0 .. "")
  print("  12  " + 0)
  print("0x", tonumber("0x"))
  print("1e", tonumber("1e"))
  print("0x1F", tonumber("0x1F"))
  print(".", tonumber("."))
  print("3x", tonumber("3x"))
  print(".5e1", tonumber(".5e1"))
  print(" 12 ", tonumber(" 12 "))
  print(21, tonumber("a", 21))
  print(28, tonumber("hirok5", 28))
  print(33, tonumber("n1mmps", 33))
  print(34, tonumber("3f", 34))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
