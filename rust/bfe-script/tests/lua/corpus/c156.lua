-- Generated differential program 156 (generate.py, seed 503).
-- patterns
do
  print(pcall(string.find, "0a[", "%x+", 1))
  print(pcall(string.find, ",x%1[y)9x.1)y%", "%b()"))
  print(pcall(string.find, ",(", "(a*)", 1))
  print(pcall(string.find, "%].(0,]", "%p", -4))
  print(pcall(string.gsub, "--z zy.b)]%c-", "(a*)", function() return 7 end, 0))
  print(pcall(string.gsub, "[ y(", "c-", function(c) return string.upper(c) end, 1))
  print(pcall(string.gsub, "-[(z%za] ] .yx%", "%b()", "#"))
  do local out = {} for a, b in string.gfind("", "x*") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
-- coerce
do
  print("10" + 5)
  print(1/0 .. "")
  print(-1/0 .. "")
  print(2^24 + 1)
  print("0x10" + 0)
  print(-"2")
  print(0.1 .. "")
  print(1e100 .. "")
  print("-7.5", tonumber("-7.5"))
  print("0x1p4", tonumber("0x1p4"))
  print("1e", tonumber("1e"))
  print("12", tonumber("12"))
  print("1e400", tonumber("1e400"))
  print("  0x10  ", tonumber("  0x10  "))
  print("3x", tonumber("3x"))
  print(23, tonumber("c", 23))
  print(13, tonumber("aa6105", 13))
  print(17, tonumber("df39c9", 17))
  print(11, tonumber("57", 11))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
