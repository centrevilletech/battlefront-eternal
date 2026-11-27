-- Generated differential program 111 (generate.py, seed 503).
-- tables
do
  local t = {-24, 3, -2, -32, 7, -34}
  print(table.getn(t), table.concat(t, ','))
  table.insert(t, -5)
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, -9)
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, 7)
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, math.max(1, math.min(2, table.getn(t) + 1)), 104)
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
  local w = {"Apple", "apple", "b", "fig", "a", "lime"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
-- errors
do
  print(pcall(error, 'plain'))
  print(pcall(error, nil))
  print(pcall(error))
  local function lvl1() error('one', 1) end local function lvl2() error('two', 2) end
  print(pcall(lvl1)) print(pcall(lvl2))
  print(pcall(function() local t = nil return t.x end))
  print(pcall(function() return undefined_global_fn() end))
  print(pcall(function() local t = {} return t.a.b end))
  print(pcall(function() return 1 < 'x' end))
  print(pcall(function() return {} .. 'x' end))
  print(pcall(function() return -{} end))
  print(pcall(function() local s = 'x' return s + 1 end))
  print(pcall(function() local t = setmetatable({}, {__index = function(t, k) error('deep ' .. k) end}) return t.key end))
  print(xpcall(function() error('boom') end, function(m) return 'handled: ' .. m end))
  print(pcall(error, setmetatable({}, {__tostring = function() return 'obj' end})) == false)
  print(pcall(assert, false, 'assert msg'))
  print(pcall(assert, nil))
  print(pcall(assert, 1, 2))
end
-- patterns
do
  print(pcall(string.find, "cc9,", "^(%w+)", -4))
  print(pcall(string.find, "aa)])%zy%", "(.)%1"))
  print(pcall(string.find, "", "^%s*(.-)%s*$", -4))
  print(pcall(string.find, "_0()90", "%x+", 1))
  print(pcall(string.find, "abbb[c90", "a?b", 20))
  print(pcall(string.find, " y)91y", "[%.%-]", 20))
  print(pcall(string.find, "99x%0y", "z$", 3))
  print(pcall(string.gsub, "]x [--bba)_a,_9.c", "^%s*(.-)%s*$", "<%0>", 1))
  print(pcall(string.gsub, "b_%[-  9.", "[%a_][%w_]*", "%%", 1))
  print(pcall(string.gsub, " )0z,a,yb[bb_(", "(.)%1", function(c) return string.upper(c) end))
  do local out = {} for a, b in string.gfind("_-b,) 1--,x][]", "%a+") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
