-- Generated differential program 125 (generate.py, seed 503).
-- patterns
do
  print(pcall(string.find, "y-0(9a0-zx1c_0", "(a*)", 20))
  print(pcall(string.find, "1azb_b[9_%_(.", "[]]"))
  print(pcall(string.find, "-c", "(.)%1", 3))
  print(pcall(string.find, "z-ccbbc[a19.]", "^%s*(.-)%s*$", 1))
  print(pcall(string.find, ",y(1(.b)", "[%.%-]", 3))
  print(pcall(string.find, "(ac]_([9(z_]  %", "[%.%-]", 1))
  print(pcall(string.find, "-)(z-%() x", "(.)%1"))
  print(pcall(string.gsub, "]bb a x9., x z)%", "[]]", "#", 1))
  print(pcall(string.gsub, "[1._a", "(%d+)", "%%", 0))
  print(pcall(string.gsub, "x.z[c", "%a+", "%%", 1))
  do local out = {} for a, b in string.gfind("%],1[,xc%", "x*") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
-- strings
do
  local s = "c9e1c"
  print(string.sub(s, 0, -12), string.sub(s, 0), string.byte(s, -12))
  print(string.sub(s, 7, 8), string.sub(s, 7), string.byte(s, 8))
  print(string.sub(s, -2, 4), string.sub(s, -2), string.byte(s, 4))
  print(string.sub(s, 5, 0), string.sub(s, 5), string.byte(s, 0))
  print(string.sub(s, 5, 5), string.sub(s, 5), string.byte(s, 5))
  print(string.sub(s, 8, -9), string.sub(s, 8), string.byte(s, -9))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("xyz", 4))
  print(string.char(107))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- recursion
do
  local function fib(k) if k < 2 then return k end return fib(k - 1) + fib(k - 2) end
  print('fib', fib(6))
  local function down(k) if k == 0 then return 'bottom' end return down(k - 1) end
  print(down(100))
  local function depth(k) if k == 0 then return 0 end return 1 + depth(k - 1) end
  print(depth(2500))
  local function ack(m, k) if m == 0 then return k + 1 elseif k == 0 then return ack(m - 1, 1) end return ack(m - 1, ack(m, k - 1)) end
  print('ack', ack(2, 4))
end
-- coerce
do
  print(1/0 .. "")
  print(0.1 .. "")
  print("10" + 5)
  print(-1/0 .. "")
  print("5" + "0.25")
  print(".5" .. 1)
  print(123456789 .. "")
  print(16777217 .. "")
  print(".5e1", tonumber(".5e1"))
  print(" 12 ", tonumber(" 12 "))
  print("0x1F", tonumber("0x1F"))
  print("+3", tonumber("+3"))
  print("5.", tonumber("5."))
  print("inf", tonumber("inf"))
  print("  0x10  ", tonumber("  0x10  "))
  print(2, tonumber("1  0", 2))
  print(26, tonumber("i5oi5", 26))
  print(17, tonumber("32333", 17))
  print(32, tonumber("l679or", 32))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
