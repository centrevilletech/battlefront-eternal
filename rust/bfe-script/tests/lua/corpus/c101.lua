-- Generated differential program 101 (generate.py, seed 503).
-- tables
do
  local t = {-46, -27, 25}
  print(table.getn(t), table.concat(t, ','))
  table.insert(t, -4)
  print(table.getn(t), table.concat(t, ' '))
  if table.getn(t) > 0 then print('removed', table.remove(t, math.min(2, table.getn(t)))) end
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
  local w = {"b", "pear", "plum", "Apple", "lime", "date"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
-- varargs
do
  local function count(...) return arg.n end
  local function pass(...) return unpack(arg) end
  print(count(), count(nil), count(1, nil), count(nil, nil, 3))
  print(pass(1, 2, 3))
  print((pass(1, 2, 3)))
  local function tail(n, ...) if n == 0 then return arg.n end return tail(n - 1, n, unpack(arg)) end
  print(tail(3))
  local a, b, c = pass()
  print(a, b, c)
  local t = {pass(1, 2), pass(3, 4)} print(table.getn(t), t[1], t[2], t[3])
end
-- compare
do
  print(35e-6 < 16777215, 35e-6 <= 16777215, 35e-6 == 16777215)
  print(9 < 2/3, 9 <= 2/3, 9 == 2/3)
  print(-23.262 < 2^-18, -23.262 <= 2^-18, -23.262 == 2^-18)
  print(-2.421 < 9, -2.421 <= 9, -2.421 == 9)
  print(25e12 < -49.868, 25e12 <= -49.868, 25e12 == -49.868)
  print(1.17549435e-38 < 2147483647, 1.17549435e-38 <= 2147483647, 1.17549435e-38 == 2147483647)
  print("B" < "A", "B" <= "A", "B" == "A")
  print("_a9" < "", "_a9" <= "", "_a9" == "")
  print("aA" < "acb", "aA" <= "acb", "aA" == "acb")
end
-- arith
do
  do local v = (276.35303)
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = ((1) / (428))
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = ((-44.735) - (((16777215) * (1e38)) * ((83e-17) + (7))))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = ((((813) + (981.56346)) - ((836) * (73.71315))) + (2^-20))
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  print('floor', math.floor(0.3))
  print('ceil', math.ceil(1))
  print('abs', math.abs(9.41358))
  print('sqrt', math.sqrt(1e10))
  do local m = math.mod(4294967295, 7) if m == m then print('mod', m) end end
  print('minmax', math.min(16e27, -33.821, 2/3), math.max(688.4333, 0.909))
  print('frexp', math.frexp(1e-40))
  print('ldexp', math.ldexp(0.75, -5))
  print('pow', 1.5 ^ -2)
end
