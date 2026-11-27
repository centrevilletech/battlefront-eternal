-- Generated differential program 021 (generate.py, seed 503).
-- patterns
do
  print(pcall(string.find, ". ),9[)x9", "(a*)", -4))
  print(pcall(string.find, "yy", "(.)%1", -4))
  print(pcall(string.find, "y.c xb)_a", "c-", 3))
  print(pcall(string.find, ")(b_b,bb.y90,", "%a+", 3))
  print(pcall(string.find, "]b](-b[", "[]]", 3))
  print(pcall(string.find, ",xca(]).", "%f[%w]%w+", 20))
  print(pcall(string.gsub, "x[0c", "z$", "#", 1))
  print(pcall(string.gsub, "ba.19_9[].9.a[", "[%a_][%w_]*", function() return false end, 2))
  print(pcall(string.gsub, "_)z,])][9", "()", function(c) return string.upper(c) end))
  do local out = {} for a, b in string.gfind("y19(% [a-0%9y-", "(%w)(%w)") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
-- coerce
do
  print("1e3" / 4)
  print(".5" .. 1)
  print(123456789 .. "")
  print(" 3e2 " * 1)
  print("0x10" + 0)
  print(1e100 .. "")
  print(1e15 .. "")
  print("5" + "0.25")
  print("  0x10  ", tonumber("  0x10  "))
  print("0x", tonumber("0x"))
  print("inf", tonumber("inf"))
  print("5.", tonumber("5."))
  print("0x1F", tonumber("0x1F"))
  print(".5e1", tonumber(".5e1"))
  print("+3", tonumber("+3"))
  print(18, tonumber("fc4", 18))
  print(31, tonumber("0 tpr", 31))
  print(34, tonumber("cb", 34))
  print(13, tonumber("0", 13))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
