-- Generated differential program 013 (generate.py, seed 503).
-- patterns
do
  print(pcall(string.find, "0[-x9z10by1c", "%x+", 20))
  print(pcall(string.find, "z.xy]_)a[(", "(%a)(%d)", -4))
  print(pcall(string.find, " b(([[yx-", "[%.%-]", 3))
  print(pcall(string.find, "_y(%,y)%,", "[^,]+"))
  print(pcall(string.find, "0y 99][_", "(%a)(%d)", 1))
  print(pcall(string.find, "0z,%(,[c([)%]", "%s"))
  print(pcall(string.gsub, "]% x.0[.(,.z[ 9", "(.)%1", function(c) return nil end, 2))
  print(pcall(string.gsub, "", "x*", function() return false end, 2))
  print(pcall(string.gsub, ",b9z.0 -)-a x)9z", "[]]", "#"))
  do local out = {} for a, b in string.gfind("9)", "(%w)(%w)") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
-- tables
do
  local t = {6, -45, 9, -12, 39}
  print(table.getn(t), table.concat(t, ','))
  table.insert(t, -1)
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, 3)
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
  local w = {"plum", "b", "a", "kiwi", "pear", "date"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
