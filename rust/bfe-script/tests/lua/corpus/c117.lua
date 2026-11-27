-- Generated differential program 117 (generate.py, seed 503).
-- closures
do
  local fs = {}
  for i = 1, 5 do fs[i] = function() return i * 7 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(4), counter(14)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
-- coerce
do
  print(16777217 .. "")
  print(1/0 .. "")
  print(" 3e2 " * 1)
  print(-0 .. "")
  print("  12  " + 0)
  print(0.1 .. "")
  print("7" * "6")
  print(3 .. "x")
  print("1 2", tonumber("1 2"))
  print("+3", tonumber("+3"))
  print("-7.5", tonumber("-7.5"))
  print("0x1p4", tonumber("0x1p4"))
  print(" 12 ", tonumber(" 12 "))
  print(".5e1", tonumber(".5e1"))
  print("1e", tonumber("1e"))
  print(32, tonumber("jq", 32))
  print(32, tonumber("q3bu", 32))
  print(29, tonumber("i 4ns", 29))
  print(8, tonumber("3 0", 8))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
