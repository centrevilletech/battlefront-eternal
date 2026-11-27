-- Generated differential program 005 (generate.py, seed 503).
-- varargs
do
  local function count(...) return arg.n end
  local function pass(...) return unpack(arg) end
  print(count(), count(nil), count(1, nil), count(nil, nil, 3))
  print(pass(1, 2, 3))
  print((pass(1, 2, 3)))
  local function tail(n, ...) if n == 0 then return arg.n end return tail(n - 1, n, unpack(arg)) end
  print(tail(3))
  local a, b, c = pass()
  print(a, b, c)
  local t = {pass(1, 2), pass(3, 4)} print(table.getn(t), t[1], t[2], t[3])
end
-- strings
do
  local s = "X X  0b1Y"
  print(string.sub(s, -2, -3), string.sub(s, -2), string.byte(s, -3))
  print(string.sub(s, -8, 13), string.sub(s, -8), string.byte(s, 13))
  print(string.sub(s, -7, -1), string.sub(s, -7), string.byte(s, -1))
  print(string.sub(s, 10, -14), string.sub(s, 10), string.byte(s, -14))
  print(string.sub(s, -6, -10), string.sub(s, -6), string.byte(s, -10))
  print(string.sub(s, 9, -14), string.sub(s, 9), string.byte(s, -14))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("-", 3))
  print(string.char(79))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- patterns
do
  print(pcall(string.find, "[%zx).%0b9x),c(a_", "c-"))
  print(pcall(string.find, "b", "(%a)(%d)", 20))
  print(pcall(string.find, "a9,zbx]%-y_", "%s", 1))
  print(pcall(string.find, ")c)yc[]( y [[-9,0", "a?b", -4))
  print(pcall(string.find, "cz0_10 ] y1", "z$", 1))
  print(pcall(string.find, "b1c]%),y0", "x*", 20))
  print(pcall(string.gsub, ")9y%]a([],", "(%d+)", function() return 7 end, 2))
  print(pcall(string.gsub, "-c.a.9),)_%", ".-b", "%%", 2))
  print(pcall(string.gsub, "_b", "%a+", "%1-", 0))
  do local out = {} for a, b in string.gfind("yb1x)yc0", "x*") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
