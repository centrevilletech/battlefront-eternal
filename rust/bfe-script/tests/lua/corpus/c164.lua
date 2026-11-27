-- Generated differential program 164 (generate.py, seed 503).
-- coerce
do
  print("0x10" + 0)
  print(".5" .. 1)
  print(-1/0 .. "")
  print(1/0 .. "")
  print(2^24 + 1)
  print("10" + 5)
  print(0.1 .. "")
  print("1e3" / 4)
  print("-7.5", tonumber("-7.5"))
  print("0x1p4", tonumber("0x1p4"))
  print("-inf", tonumber("-inf"))
  print("0x", tonumber("0x"))
  print("+3", tonumber("+3"))
  print(".5e1", tonumber(".5e1"))
  print("12", tonumber("12"))
  print(4, tonumber("1", 4))
  print(6, tonumber("224", 6))
  print(11, tonumber("642", 11))
  print(32, tonumber("d6570k", 32))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- tables
do
  local t = {8, 2, 36, -26, 4, 26, -6, -41, -27}
  print(table.getn(t), table.concat(t, ','))
  table.insert(t, -2)
  print(table.getn(t), table.concat(t, ' '))
  table.insert(t, -9)
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
  local w = {"lime", "Apple", "pear", "kiwi", "a", "plum"} table.sort(w) print(table.concat(w, ' '))
  for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end
end
-- patterns
do
  print(pcall(string.find, "[0).xx9,(1", "(%d+)", 20))
  print(pcall(string.find, "a),.zy[[x[%z],b_1", "(a*)", 3))
  print(pcall(string.find, "]c_b_(_1x.a.x( ", "[]]", 20))
  print(pcall(string.find, "0%0,,x", "z$", 1))
  print(pcall(string.find, "(x,(0-", "%a+"))
  print(pcall(string.find, "0)z ", "[^%s]+"))
  print(pcall(string.gsub, ")1b,y", "[%a_][%w_]*", function() return false end))
  print(pcall(string.gsub, "[a0._xa9[9x%x]1", "^(%w+)", "<%0>"))
  print(pcall(string.gsub, "(.1y%)bx)]][c,1,_", "%f[%w]%w+", "%%", 0))
  do local out = {} for a, b in string.gfind("]10]9( [", "x*") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
-- loops
do
  do local n, last = 0, nil for i = 0.5, 2.6, 2 do n = n + 1 last = i if n > 50 then break end end print('steps', n, last) end
  do local s = 0 for i = 1, 11 do if math.mod(i, 3) == 0 then s = s + i end end print('sum', s) end
  do local k = 0 repeat k = k + 2 until k >= 10 print('repeat', k) end
  do local x = 0.1 local m = 0 while x < 1 do x = x + 0.1 m = m + 1 end print('float loop', m, x) end
  do local t = {} for i = 1, 5 do t[i] = i * i end local s = '' for i, v in ipairs(t) do s = s .. v .. ';' end print(s) end
end
