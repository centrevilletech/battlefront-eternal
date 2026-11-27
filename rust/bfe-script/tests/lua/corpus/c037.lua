-- Generated differential program 037 (generate.py, seed 503).
-- compare
do
  print(0.47 < 33e-37, 0.47 <= 33e-37, 0.47 == 33e-37)
  print(16777216 < 5e-17, 16777216 <= 5e-17, 16777216 == 5e-17)
  print(0.956098161 < -653, 0.956098161 <= -653, 0.956098161 == -653)
  print(2^-1 < 4, 2^-1 <= 4, 2^-1 == 4)
  print("a" < "b", "a" <= "b", "a" == "b")
  print("B" < "bc", "B" <= "bc", "B" == "bc")
  print("9" < "", "9" <= "", "9" == "")
end
-- strings
do
  local s = "099"
  print(string.sub(s, -3, 1), string.sub(s, -3), string.byte(s, 1))
  print(string.sub(s, 9, 11), string.sub(s, 9), string.byte(s, 11))
  print(string.sub(s, -6, -3), string.sub(s, -6), string.byte(s, -3))
  print(string.sub(s, 8, -7), string.sub(s, 8), string.byte(s, -7))
  print(string.sub(s, 1, -14), string.sub(s, 1), string.byte(s, -14))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("", 1))
  print(string.char(54, 88, 110, 75, 84))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- tables
do
  local t = {4, -13, -45, -29, -50, -48, -6}
  print(table.getn(t), table.concat(t, ','))
  if table.getn(t) > 0 then print('removed', table.remove(t, math.min(3, table.getn(t)))) end
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, 2)
  print(table.getn(t), table.concat(t, ' '))
  if table.getn(t) > 0 then print('removed', table.remove(t, math.min(1, table.getn(t)))) end
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, math.max(1, math.min(2, table.getn(t) + 1)), 186)
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
  local w = {"a", "Apple", "fig", "plum", "kiwi", "b"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
