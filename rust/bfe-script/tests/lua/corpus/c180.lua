-- Generated differential program 180 (generate.py, seed 503).
-- patterns
do
  print(pcall(string.find, "z1", "(%a)(%d)", -4))
  print(pcall(string.find, "]%yxx(1%,-x by_(", "^%s*(.-)%s*$", 3))
  print(pcall(string.find, "y-)x_a[(]% 0", "(.)%1", 20))
  print(pcall(string.find, " ]9a[[z%abyb,x-xb", "(b+)(c*)"))
  print(pcall(string.gsub, "-y9_]xc_)[(", "%p", function() return 7 end))
  print(pcall(string.gsub, ".,19xyz9c9y-z", "(%d+)", function() return false end))
  print(pcall(string.gsub, ".b)0", "z$", "#", 0))
  do local out = {} for a, b in string.gfind(".zyz,(y%,", "[,.]") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
-- trig
do
  print('sin', math.sin(0.001))
  print('rad', math.rad(0.25))
  print('log', math.log(0.25))
  print('atan', math.atan(1))
  print('atan2', math.atan2(-1, 1))
  print('pi', math.pi, math.pi * 2)
end
-- strings
do
  local s = "b01e1af"
  print(string.sub(s, -5, -7), string.sub(s, -5), string.byte(s, -7))
  print(string.sub(s, 12, -3), string.sub(s, 12), string.byte(s, -3))
  print(string.sub(s, 9, 12), string.sub(s, 9), string.byte(s, 12))
  print(string.sub(s, 8, -1), string.sub(s, 8), string.byte(s, -1))
  print(string.sub(s, -15, -11), string.sub(s, -15), string.byte(s, -11))
  print(string.sub(s, -1, -4), string.sub(s, -1), string.byte(s, -4))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("-", 3))
  print(string.char(78, 40, 72))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
