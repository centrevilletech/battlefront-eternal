-- Generated differential program 032 (generate.py, seed 503).
-- coerce
do
  print("7" * "6")
  print("0x10" + 0)
  print("  12  " + 0)
  print(16777217 .. "")
  print(123456789 .. "")
  print(0.1 .. "")
  print(-"2")
  print(-1/0 .. "")
  print("-7.5", tonumber("-7.5"))
  print("  0x10  ", tonumber("  0x10  "))
  print("1e400", tonumber("1e400"))
  print("5.", tonumber("5."))
  print("+3", tonumber("+3"))
  print("1e", tonumber("1e"))
  print("0x1F", tonumber("0x1F"))
  print(29, tonumber("pc1c5", 29))
  print(22, tonumber("d23", 22))
  print(11, tonumber("7801", 11))
  print(19, tonumber("ae2b", 19))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- tables
do
  local t = {43, -1, 14, 26}
  print(table.getn(t), table.concat(t, ','))
  table.insert(t, 2)
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, math.max(1, math.min(2, table.getn(t) + 1)), 183)
  print(table.getn(t), table.concat(t, ' '))
  if table.getn(t) > 0 then print('removed', table.remove(t)) end
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
  local w = {"b", "a", "lime", "kiwi", "pear", "apple"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
-- arith
do
  do local v = ((((0.25754) / (830)) + (0.538400094)) + (1/3))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = ((((488) + (0.1)) * ((-32.963) + (88.38280))) + (((253) - (6)) - (8388609)))
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = (2147483647)
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = ((((162) + (349.60629)) + (60e14)) / ((4294967295) + (0.2)))
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  print('floor', math.floor(0.1))
  print('ceil', math.ceil(79.33965))
  print('abs', math.abs(633.92634))
  print('sqrt', math.sqrt(2))
  do local m = math.mod(544.71646, 7) if m == m then print('mod', m) end end
  print('minmax', math.min(0.683074072, 53e-38, 601.16873), math.max(63e-1, 746.23207))
  print('frexp', math.frexp(8))
  print('ldexp', math.ldexp(1, 17))
  print('pow', 1.5 ^ 3)
end
