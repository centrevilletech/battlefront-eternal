-- Generated differential program 034 (generate.py, seed 503).
-- format
do
  print(pcall(string.format, "[%q]", "nul\0byte"))
  print(pcall(string.format, "[% -o]", -98765))
  print(pcall(string.format, "[%#9i]", 255))
  print(pcall(string.format, "[%+u]", 65535))
  print(pcall(string.format, "[%#4x]", 123456))
  print(pcall(string.format, "[%17.5s]", "xxxxxxxxxxxx"))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
-- patterns
do
  print(pcall(string.find, "a", "(a*)", 20))
  print(pcall(string.find, "cb1", "%f[%w]%w+"))
  print(pcall(string.find, "x", "z$", 1))
  print(pcall(string.find, "[ 0,)z9)-1", "%a+"))
  print(pcall(string.find, "x,b1%,-]ya.y,9[)", "[^%s]+", 3))
  print(pcall(string.find, "0a]9, . (.c(c9", "(%a)(%d)"))
  print(pcall(string.gsub, ",[(z[]x-", "%a+", "#"))
  print(pcall(string.gsub, "-[(1)bzyb", "%f[%w]%w+", "<%0>"))
  print(pcall(string.gsub, "y c_)](bb", "%f[%w]%w+", function(c) return string.upper(c) end, 1))
  do local out = {} for a, b in string.gfind("%%b%", "[,.]") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
-- arith
do
  do local v = ((((0.1) / (2147483647)) * ((6) * (2^4))) * (((-646) + (-925)) * (0.72898272)))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = (1e38)
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = (((-839) * ((107.18612) - (-9.0))) * (((98e-13) * (-15.21)) * (1/3)))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = (((468) * (-12.185)) + (16777217))
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = (2^27)
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  do local v = ((0.05579733) * (((0.5792377439) + (429)) - ((-28.35) / (0.8602))))
    if v == v then print(5, v, v * 2, -v) else print(5, 'not a number') end end
  do local v = ((((349.46878) - (1e38)) * (2^16)) - (2^-5))
    if v == v then print(6, v, v * 2, -v) else print(6, 'not a number') end end
  do local v = ((((2) * (998.98206)) + (7e19)) * ((8388609) / (4)))
    if v == v then print(7, v, v * 2, -v) else print(7, 'not a number') end end
  print('floor', math.floor(2^-7))
  print('ceil', math.ceil(39e-3))
  print('abs', math.abs(0.1))
  print('sqrt', math.sqrt(1e10))
  do local m = math.mod(86e29, 7) if m == m then print('mod', m) end end
  print('minmax', math.min(484.7689, 2^-5, 7), math.max(439.39398, -29.410))
  print('frexp', math.frexp(-5))
  print('ldexp', math.ldexp(0.75, 1))
  print('pow', 10 ^ 0.5)
end
