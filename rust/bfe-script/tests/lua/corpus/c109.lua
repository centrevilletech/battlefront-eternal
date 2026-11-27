-- Generated differential program 109 (generate.py, seed 503).
-- closures
do
  local fs = {}
  for i = 1, 3 do fs[i] = function() return i * 6 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(3), counter(17)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
-- tables
do
  local t = {17, 17, 39, 32, -37, 32, 22, 43}
  print(table.getn(t), table.concat(t, ','))
  if table.getn(t) > 0 then print('removed', table.remove(t)) end
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, math.max(1, math.min(4, table.getn(t) + 1)), 123)
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, -8)
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, 5)
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
  local w = {"date", "plum", "Apple", "a", "apple", "fig"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
-- compare
do
  print(16e37 < 82e-2, 16e37 <= 82e-2, 16e37 == 82e-2)
  print(45e-21 < 0.547023812, 45e-21 <= 0.547023812, 45e-21 == 0.547023812)
  print(5 < 2^26, 5 <= 2^26, 5 == 2^26)
  print(14e24 < 0.694311, 14e24 <= 0.694311, 14e24 == 0.694311)
  print(2147483647 < 2^-27, 2147483647 <= 2^-27, 2147483647 == 2^-27)
  print("aab" < "_B_A", "aab" <= "_B_A", "aab" == "_B_A")
  print("" < "AB", "" <= "AB", "" == "AB")
  print("9" < "bA", "9" <= "bA", "9" == "bA")
end
-- arith
do
  do local v = ((((7) + (16777216)) + ((1) / (2^3))) * (((998.4884) - (5)) * (-6.38)))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = (41e-35)
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = (((-124) * (144.20757)) / (((-255) * (16777217)) * (-10.890)))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = (0.553391)
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = (-782)
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  print('floor', math.floor(1/3))
  print('ceil', math.ceil(-7.233))
  print('abs', math.abs(6))
  print('sqrt', math.sqrt(1e10))
  do local m = math.mod(4, 2.5) if m == m then print('mod', m) end end
  print('minmax', math.min(0.33995, 2, -886), math.max(6, -24.331))
  print('frexp', math.frexp(1))
  print('ldexp', math.ldexp(0.75, -3))
  print('pow', 0.5 ^ 10)
end
