-- Generated differential program 018 (generate.py, seed 503).
-- strings
do
  local s = "90 "
  print(string.sub(s, 13, 15), string.sub(s, 13), string.byte(s, 15))
  print(string.sub(s, -9, 11), string.sub(s, -9), string.byte(s, 11))
  print(string.sub(s, 3, -7), string.sub(s, 3), string.byte(s, -7))
  print(string.sub(s, 1, 15), string.sub(s, 1), string.byte(s, 15))
  print(string.sub(s, 11, 2), string.sub(s, 11), string.byte(s, 2))
  print(string.sub(s, -6, 0), string.sub(s, -6), string.byte(s, 0))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("-", 4))
  print(string.char(106, 73))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- tables
do
  local t = {49, -28, 14}
  print(table.getn(t), table.concat(t, ','))
  if table.getn(t) > 0 then print('removed', table.remove(t)) end
  print(table.getn(t), table.concat(t, ' '))
  if table.getn(t) > 0 then print('removed', table.remove(t, math.min(3, table.getn(t)))) end
  print(table.getn(t), table.concat(t, ' '))
  if table.getn(t) > 0 then print('removed', table.remove(t, math.min(3, table.getn(t)))) end
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, math.max(1, math.min(4, table.getn(t) + 1)), 116)
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
  local w = {"kiwi", "date", "a", "b", "fig", "lime"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
-- coerce
do
  print(1e100 .. "")
  print(1/0 .. "")
  print(2^24 + 1)
  print(0.1 .. "")
  print(".5" .. 1)
  print(123456789 .. "")
  print("7" * "6")
  print(10 .. 20)
  print("0x1p4", tonumber("0x1p4"))
  print("5.", tonumber("5."))
  print("3x", tonumber("3x"))
  print("-inf", tonumber("-inf"))
  print("  0x10  ", tonumber("  0x10  "))
  print("1 2", tonumber("1 2"))
  print("1e", tonumber("1e"))
  print(13, tonumber("ba1641", 13))
  print(16, tonumber("71570", 16))
  print(28, tonumber("8rjp", 28))
  print(11, tonumber("018 78", 11))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
