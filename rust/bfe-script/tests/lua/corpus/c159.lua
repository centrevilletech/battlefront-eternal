-- Generated differential program 159 (generate.py, seed 503).
-- tables
do
  local t = {-21, -31, 7, 28, 29}
  print(table.getn(t), table.concat(t, ','))
  if table.getn(t) > 0 then print('removed', table.remove(t)) end
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
  local w = {"plum", "a", "lime", "kiwi", "date", "b"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
-- coerce
do
  print("  12  " + 0)
  print(2^24 + 1)
  print(-1/0 .. "")
  print(1/0 .. "")
  print(123456789 .. "")
  print(-"2")
  print(3 .. "x")
  print(1e100 .. "")
  print(".5e1", tonumber(".5e1"))
  print("  0x10  ", tonumber("  0x10  "))
  print("1e400", tonumber("1e400"))
  print("0x1F", tonumber("0x1F"))
  print("inf", tonumber("inf"))
  print("+3", tonumber("+3"))
  print("12", tonumber("12"))
  print(18, tonumber("a66", 18))
  print(3, tonumber("0", 3))
  print(11, tonumber("74", 11))
  print(26, tonumber("92h", 26))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- trig
do
  print('tan', math.tan(0.001))
  print('rad', math.rad(1))
  print('atan', math.atan(10))
  print('log', math.log(3))
  print('atan2', math.atan2(-1, 1))
  print('pi', math.pi, math.pi * 2)
end
