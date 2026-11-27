-- Generated differential program 225 (generate.py, seed 503).
-- tables
do
  local t = {-20, 43, 5}
  print(table.getn(t), table.concat(t, ','))
  if table.getn(t) > 0 then print('removed', table.remove(t, math.min(1, table.getn(t)))) end
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, math.max(1, math.min(1, table.getn(t) + 1)), 187)
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
  local w = {"plum", "pear", "date", "lime", "a", "apple"} table.sort(w) print(table.concat(w, ' '))
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
  local a, b, c = pass(3, 7, 8, 5)
  print(a, b, c)
  local t = {pass(1, 2), pass(3, 4)} print(table.getn(t), t[1], t[2], t[3])
end
-- patterns
do
  print(pcall(string.find, "zz,)bxa]0c1z 0-x", "[]]", -4))
  print(pcall(string.find, "b(9", "%b()", -4))
  print(pcall(string.find, "", "^%s*(.-)%s*$", 1))
  print(pcall(string.find, "y-z_,x[9]..", "[]]", 20))
  print(pcall(string.find, "b90_1y-1x[(", "^(%w+)"))
  print(pcall(string.gsub, "zy[-90", "[%.%-]", "%%"))
  print(pcall(string.gsub, "- [c)a9[11c", "x*", function(c) return string.upper(c) end))
  print(pcall(string.gsub, "0x1 cbxbz9", "[%a_][%w_]*", function(c) return string.upper(c) end, 2))
  do local out = {} for a, b in string.gfind("z)x_c -]]-aa_(_)(", "%d") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
