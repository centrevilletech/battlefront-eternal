-- Generated differential program 112 (generate.py, seed 503).
-- closures
do
  local fs = {}
  for i = 1, 2 do fs[i] = function() return i * 1 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(9), counter(19)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
-- tables
do
  local t = {17, -36, -43, 11, -40, 27, -9}
  print(table.getn(t), table.concat(t, ','))
  if table.getn(t) > 0 then print('removed', table.remove(t, math.min(3, table.getn(t)))) end
  print(table.getn(t), table.concat(t, ' '))
  if table.getn(t) > 0 then print('removed', table.remove(t, math.min(1, table.getn(t)))) end
  print(table.getn(t), table.concat(t, ' '))
  if table.getn(t) > 0 then print('removed', table.remove(t, math.min(3, table.getn(t)))) end
  print(table.getn(t), table.concat(t, ' '))
  table.sort(t)
  print(table.concat(t, ' '))
  table.sort(t, function(a, b) return a > b end)
  print(table.concat(t, ' '))
  print(pcall(table.concat, t, '-', 2, 3))
  print(table.concat({}, 'x'), table.concat(t, '', 3, 2))
  local u = {n = 2, 'a', 'b', 'c'} print(table.getn(u))
  table.setn(u, 1) print(table.getn(u))
  print(unpack({1, 2, nil, 4}))
  table.foreachi(t, function(i, v) if i <= 3 then print(i, v) end end)
  local w = {"date", "fig", "apple", "a", "lime", "kiwi"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
-- coerce
do
  print(1/0 .. "")
  print(" 3e2 " * 1)
  print("1e3" / 4)
  print("  12  " + 0)
  print(123456789 .. "")
  print(3 .. "x")
  print("0x10" + 0)
  print("10" + 5)
  print("  0x10  ", tonumber("  0x10  "))
  print("1e400", tonumber("1e400"))
  print("-inf", tonumber("-inf"))
  print("-7.5", tonumber("-7.5"))
  print("1e", tonumber("1e"))
  print("0x1F", tonumber("0x1F"))
  print("+3", tonumber("+3"))
  print(5, tonumber("433", 5))
  print(18, tonumber("7", 18))
  print(17, tonumber("5", 17))
  print(29, tonumber("6", 29))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
