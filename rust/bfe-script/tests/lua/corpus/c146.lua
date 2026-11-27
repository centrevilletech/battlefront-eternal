-- Generated differential program 146 (generate.py, seed 503).
-- strings
do
  local s = "ffXeX"
  print(string.sub(s, 2, -12), string.sub(s, 2), string.byte(s, -12))
  print(string.sub(s, 13, 4), string.sub(s, 13), string.byte(s, 4))
  print(string.sub(s, -1, -8), string.sub(s, -1), string.byte(s, -8))
  print(string.sub(s, 4, 1), string.sub(s, 4), string.byte(s, 1))
  print(string.sub(s, -10, 6), string.sub(s, -10), string.byte(s, 6))
  print(string.sub(s, 7, 15), string.sub(s, 7), string.byte(s, 15))
  print(string.sub(s, -9, -12), string.sub(s, -9), string.byte(s, -12))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("", 0))
  print(string.char(112))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- patterns
do
  print(pcall(string.find, "_)%]a)0x[-b_]a-.", "^(%w+)", -4))
  print(pcall(string.find, "zy%(", ".-b", 3))
  print(pcall(string.find, "%x zb].", "[%a_][%w_]*", 1))
  print(pcall(string.find, "xy", "%p", 3))
  print(pcall(string.gsub, "_", "(.)%1", function(c) return string.upper(c) end))
  print(pcall(string.gsub, " ", "a?b", function() return false end, 1))
  print(pcall(string.gsub, "9)a0x[(b_.x[ zzzb", "^%s*(.-)%s*$", "%%"))
  do local out = {} for a, b in string.gfind("z9%_xc0%yb", "%a+") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
