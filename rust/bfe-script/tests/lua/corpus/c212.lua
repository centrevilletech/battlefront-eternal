-- Generated differential program 212 (generate.py, seed 503).
-- arith
do
  do local v = ((0.264) / (57e-26))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = ((((2147483647) * (8)) * (0.3)) + (((7e33) - (2^7)) * ((-33.458) + (2^14))))
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = ((((2^-17) / (42e6)) * ((1e-45) * (1))) / (2^19))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = (0.78916)
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = (((98e-38) + ((-14.924) + (18e-12))) + (((0.3) + (0.58)) - ((0.3) + (-193))))
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  do local v = ((142.93362) * (((2^9) * (2^16)) + (0.3557674959)))
    if v == v then print(5, v, v * 2, -v) else print(5, 'not a number') end end
  do local v = ((16777217) * (60e-17))
    if v == v then print(6, v, v * 2, -v) else print(6, 'not a number') end end
  print('floor', math.floor(33554433))
  print('ceil', math.ceil(0.12))
  print('abs', math.abs(333.18313))
  print('sqrt', math.sqrt(1e10))
  do local m = math.mod(2147483647, 2.5) if m == m then print('mod', m) end end
  print('minmax', math.min(2/3, 16777217, 479.32397), math.max(26e-19, 2147483647))
  print('frexp', math.frexp(8))
  print('ldexp', math.ldexp(-3, -19))
  print('pow', 0.5 ^ -2)
end
-- format
do
  print(pcall(string.format, "[%5.2s]", "hello"))
  print(pcall(string.format, "[%q]", "line1\nline2"))
  print(pcall(string.format, "[%.1E]", 6))
  print(pcall(string.format, "[%+14g]", 8388609))
  print(pcall(string.format, "[%2s]", "tab\9here"))
  print(pcall(string.format, "[%c]", 65))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
