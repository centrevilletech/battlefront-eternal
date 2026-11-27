-- Generated differential program 012 (generate.py, seed 503).
-- format
do
  print(pcall(string.format, "[%3.11s]", "hello"))
  print(pcall(string.format, "[%0#16.13G]", 611.72386))
  print(pcall(string.format, "[%#17f]", -6.116))
  print(pcall(string.format, "[%0+11E]", 393.16095))
  print(pcall(string.format, "[%i]", 65535))
  print(pcall(string.format, "[%#x]", -1))
  print(pcall(string.format, "[%1c]", 65))
  print(pcall(string.format, "[%X]", -98765))
  print(pcall(string.format, "[%15g]", 832.45539))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
-- patterns
do
  print(pcall(string.find, "x- 9", "%b()", 1))
  print(pcall(string.find, "1_ ]09ayz1c,b", "[%a_][%w_]*"))
  print(pcall(string.find, "yz%)9 ", "()", 20))
  print(pcall(string.find, "9x(", "[^%s]+", -4))
  print(pcall(string.gsub, "%)zy[b b", "[%a_][%w_]*", "<%0>", 2))
  print(pcall(string.gsub, "[y,, (z]9cy.%,1xb", "%p", function(c) return string.upper(c) end, 2))
  print(pcall(string.gsub, "cy%01, x.(ba", "z$", function(c) return nil end, 2))
  do local out = {} for a, b in string.gfind(".1", "%a+") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
-- coerce
do
  print(-"2")
  print("0x10" + 0)
  print(10 .. 20)
  print("1e3" / 4)
  print(2^24 + 1)
  print("7" * "6")
  print(1e15 .. "")
  print(3 .. "x")
  print(".5e1", tonumber(".5e1"))
  print("  0x10  ", tonumber("  0x10  "))
  print("1e400", tonumber("1e400"))
  print("0x", tonumber("0x"))
  print("5.", tonumber("5."))
  print("0x1p4", tonumber("0x1p4"))
  print("inf", tonumber("inf"))
  print(12, tonumber("2", 12))
  print(21, tonumber("hf5", 21))
  print(27, tonumber("k5", 27))
  print(20, tonumber("i", 20))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
