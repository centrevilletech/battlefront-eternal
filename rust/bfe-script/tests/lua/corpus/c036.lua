-- Generated differential program 036 (generate.py, seed 503).
-- tables
do
  local t = {30, 27, 13, -35, -29, -17, 8, 33}
  print(table.getn(t), table.concat(t, ','))
  table.insert(t, 4)
  print(table.getn(t), table.concat(t, ' '))
  if table.getn(t) > 0 then print('removed', table.remove(t)) end
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
  local w = {"date", "kiwi", "apple", "lime", "a", "fig"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
-- coerce
do
  print(-0 .. "")
  print(-1/0 .. "")
  print(" 3e2 " * 1)
  print(1e100 .. "")
  print(3 .. "x")
  print(-"2")
  print(1/0 .. "")
  print("0x10" + 0)
  print("1e400", tonumber("1e400"))
  print("  0x10  ", tonumber("  0x10  "))
  print("-inf", tonumber("-inf"))
  print("+3", tonumber("+3"))
  print("12", tonumber("12"))
  print("0x1p4", tonumber("0x1p4"))
  print("inf", tonumber("inf"))
  print(3, tonumber("011", 3))
  print(22, tonumber("8e", 22))
  print(32, tonumber("92e", 32))
  print(2, tonumber("0 1110", 2))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- patterns
do
  print(pcall(string.find, "-bb)990_1y%)y,", "(b+)(c*)", 1))
  print(pcall(string.find, "a[", "(.)%1", -4))
  print(pcall(string.find, "x9,c1%y[c[z()[1_", "[^%s]+", 3))
  print(pcall(string.find, "c._b,c9%_.y)zc%]", "^%s*(.-)%s*$", 20))
  print(pcall(string.gsub, "_c-z_a", "%b()", "<%0>", 2))
  print(pcall(string.gsub, "1", "(b+)(c*)", function(c) return string.upper(c) end, 1))
  print(pcall(string.gsub, "azx1c.", "%a+", function() return false end, 0))
  do local out = {} for a, b in string.gfind("0c1(-%]c.[", "(%w)(%w)") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
