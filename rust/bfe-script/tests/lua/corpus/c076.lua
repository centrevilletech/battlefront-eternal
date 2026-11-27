-- Generated differential program 076 (generate.py, seed 503).
-- coerce
do
  print("7" * "6")
  print(1/0 .. "")
  print("  12  " + 0)
  print(3 .. "x")
  print(1e15 .. "")
  print(-1/0 .. "")
  print(1e100 .. "")
  print(123456789 .. "")
  print("1 2", tonumber("1 2"))
  print("inf", tonumber("inf"))
  print("-7.5", tonumber("-7.5"))
  print("0x1F", tonumber("0x1F"))
  print("1e", tonumber("1e"))
  print("  0x10  ", tonumber("  0x10  "))
  print("0x", tonumber("0x"))
  print(23, tonumber("4 7k", 23))
  print(31, tonumber("80", 31))
  print(30, tonumber("8r 0", 30))
  print(11, tonumber("3", 11))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- patterns
do
  print(pcall(string.find, "a1(]", "z$", 20))
  print(pcall(string.find, ".99bb_(x(", "[%.%-]", 1))
  print(pcall(string.find, "[c]-c9[_0a,(ay", "a?b", -4))
  print(pcall(string.find, "[z0 %.", "[]]", -4))
  print(pcall(string.gsub, "-]x%(%._", "%f[%w]%w+", function(c) return nil end, 1))
  print(pcall(string.gsub, "%.c99-9y9cc[9", ".-b", "%%", 2))
  print(pcall(string.gsub, "(].1, (1%0", "a?b", function() return 7 end, 0))
  do local out = {} for a, b in string.gfind("0", "[,.]") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
-- compare
do
  print(0.0798 < -90, 0.0798 <= -90, 0.0798 == -90)
  print(2^16 < 35e-8, 2^16 <= 35e-8, 2^16 == 35e-8)
  print(6 < 0.86669, 6 <= 0.86669, 6 == 0.86669)
  print(-583 < 0.321, -583 <= 0.321, -583 == 0.321)
  print(16777215 < 16777216, 16777215 <= 16777216, 16777215 == 16777216)
  print(9 < 16777217, 9 <= 16777217, 9 == 16777217)
  print("9Ba" < "09B_", "9Ba" <= "09B_", "9Ba" == "09B_")
  print("" < "0bB", "" <= "0bB", "" == "0bB")
  print("A" < "0", "A" <= "0", "A" == "0")
end
