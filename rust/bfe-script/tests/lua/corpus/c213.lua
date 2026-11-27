-- Generated differential program 213 (generate.py, seed 503).
-- patterns
do
  print(pcall(string.find, "0ax]_x_a0x[ 9", "%s", 20))
  print(pcall(string.find, ")[1", "[%a_][%w_]*", 3))
  print(pcall(string.find, "a%%,1)1(,-,% ", "[^%s]+"))
  print(pcall(string.find, "0. ,_01", "[^,]+", -4))
  print(pcall(string.find, "_z", "(a*)", 3))
  print(pcall(string.gsub, "xc(ax]x", "%b()", function(c) return string.upper(c) end, 0))
  print(pcall(string.gsub, "by -b", "(a*)", "%1-", 1))
  print(pcall(string.gsub, "", "c-", function(c) return string.upper(c) end, 2))
  do local out = {} for a, b in string.gfind("9aa(__9_)", "[,.]") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
-- arith
do
  do local v = (2^3)
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = (2147483647)
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = ((-840) * (((-136) * (2147483647)) + ((1e38) - (0.8548))))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = ((((0.0678659330) * (9)) + (2147483647)) + (((315.1242) * (3.4e38)) * ((1e38) / (2/3))))
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = (((33554433) * ((4294967295) - (-10.403))) - (((-537) * (9)) + ((-973) - (21.48929))))
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  do local v = (((-6.0) - ((17e14) * (0.93525))) * (0.2))
    if v == v then print(5, v, v * 2, -v) else print(5, 'not a number') end end
  print('floor', math.floor(10.64112))
  print('ceil', math.ceil(-35.590))
  print('abs', math.abs(7e-30))
  print('sqrt', math.sqrt(1e10))
  do local m = math.mod(59.49418, 7) if m == m then print('mod', m) end end
  print('minmax', math.min(-47.245, 8388609, 71e-37), math.max(41e-40, 16777217))
  print('frexp', math.frexp(0.3))
  print('ldexp', math.ldexp(1, -1))
  print('pow', 2 ^ -2)
end
