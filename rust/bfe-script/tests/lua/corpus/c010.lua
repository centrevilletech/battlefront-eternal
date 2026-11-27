-- Generated differential program 010 (generate.py, seed 503).
-- format
do
  print(pcall(string.format, "[%5c]", 126))
  print(pcall(string.format, "[%-u]", -1))
  print(pcall(string.format, "[%#+10i]", -98765))
  print(pcall(string.format, "[% +G]", 2^-2))
  print(pcall(string.format, "[%q]", "plain"))
  print(pcall(string.format, "[%5.2s]", "a b"))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
-- patterns
do
  print(pcall(string.find, "x]((-ya[", "[^,]+", 1))
  print(pcall(string.find, "(0)]cax(%%b0]x)a", "%f[%w]%w+", 20))
  print(pcall(string.find, "9.,0a-", "(b+)(c*)", 20))
  print(pcall(string.find, " 1by]", "(a*)", 3))
  print(pcall(string.find, ")az[-[., c90ax", "^%s*(.-)%s*$", 3))
  print(pcall(string.find, " ", ".-b"))
  print(pcall(string.gsub, ",y(_%(%].%011a%xx", "(%a)(%d)", "%1-", 2))
  print(pcall(string.gsub, "]cy-%9].xb0009-_", "()", function(c) return string.upper(c) end, 0))
  print(pcall(string.gsub, "ax_", "[^%s]+", "%1-", 1))
  do local out = {} for a, b in string.gfind("]", "[,.]") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
