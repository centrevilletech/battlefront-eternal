-- Generated differential program 137 (generate.py, seed 503).
-- strings
do
  local s = "9f09 bc bZ"
  print(string.sub(s, 15, 2), string.sub(s, 15), string.byte(s, 2))
  print(string.sub(s, -5, 3), string.sub(s, -5), string.byte(s, 3))
  print(string.sub(s, -12, 4), string.sub(s, -12), string.byte(s, 4))
  print(string.sub(s, 5, 5), string.sub(s, 5), string.byte(s, 5))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("ab", 1))
  print(string.char(43))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- coerce
do
  print("0x10" + 0)
  print(1/0 .. "")
  print(16777217 .. "")
  print("5" + "0.25")
  print(3 .. "x")
  print(10 .. 20)
  print("1e3" / 4)
  print(0.1 .. "")
  print("3x", tonumber("3x"))
  print("0x1F", tonumber("0x1F"))
  print("+3", tonumber("+3"))
  print("  0x10  ", tonumber("  0x10  "))
  print("-inf", tonumber("-inf"))
  print(".5e1", tonumber(".5e1"))
  print("12", tonumber("12"))
  print(6, tonumber("04", 6))
  print(28, tonumber("3j59", 28))
  print(10, tonumber("9", 10))
  print(34, tonumber("e", 34))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- tables
do
  local t = {14, 33, 7, 19, 48, -36, -8, 46, 36}
  print(table.getn(t), table.concat(t, ','))
  if table.getn(t) > 0 then print('removed', table.remove(t, math.min(1, table.getn(t)))) end
  print(table.getn(t), table.concat(t, ' '))
  if table.getn(t) > 0 then print('removed', table.remove(t)) end
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, 2)
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
  local w = {"Apple", "a", "fig", "kiwi", "b", "lime"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
-- patterns
do
  print(pcall(string.find, "0z_", "z$", 1))
  print(pcall(string.find, "_9_11zcx_]_y(", "(%a)(%d)", 1))
  print(pcall(string.find, "c-z]zx", ".-b", 20))
  print(pcall(string.find, "_.%c)y),-%x- [(", "x*", 1))
  print(pcall(string.find, "0  ,axaz-1,", "%f[%w]%w+", 20))
  print(pcall(string.find, "bcy1[", "()"))
  print(pcall(string.find, " -. y0],ac[%]_,(_", "^%s*(.-)%s*$", 3))
  print(pcall(string.gsub, "z]%az9a.x,.-(),].", "%p", function(c) return nil end, 0))
  print(pcall(string.gsub, "[ czxca_]", "[]]", function() return false end, 2))
  print(pcall(string.gsub, "0cx%1(00.-_x990c_", "%b()", function(c) return string.upper(c) end, 1))
  do local out = {} for a, b in string.gfind(",0]b-,zb_,]", "(%w)(%w)") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
