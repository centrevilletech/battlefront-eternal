-- Generated differential program 085 (generate.py, seed 503).
-- loops
do
  do local n, last = 0, nil for i = -2, 2.6, -1 do n = n + 1 last = i if n > 50 then break end end print('steps', n, last) end
  do local s = 0 for i = 1, 6 do if math.mod(i, 3) == 0 then s = s + i end end print('sum', s) end
  do local k = 0 repeat k = k + 2 until k >= 9 print('repeat', k) end
  do local x = 0.1 local m = 0 while x < 1 do x = x + 0.1 m = m + 1 end print('float loop', m, x) end
  do local t = {} for i = 1, 5 do t[i] = i * i end local s = '' for i, v in ipairs(t) do s = s .. v .. ';' end print(s) end
end
-- arith
do
  do local v = (2^-21)
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = ((((3.4e38) - (4294967295)) / (0.1)) * (((0.50045) * (5)) * ((16777217) * (-21.286))))
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = ((2^14) + ((0.1) * ((2147483647) * (283))))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = (79e-4)
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = (((36e-39) * ((2^11) - (0.7))) * ((0.82) - ((829) + (680.64631))))
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  do local v = ((1e-45) * (((16777217) + (179.45056)) + ((2^-15) * (2))))
    if v == v then print(5, v, v * 2, -v) else print(5, 'not a number') end end
  do local v = ((((1e38) / (-429)) * (8)) - (-883))
    if v == v then print(6, v, v * 2, -v) else print(6, 'not a number') end end
  print('floor', math.floor(2147483647))
  print('ceil', math.ceil(8))
  print('abs', math.abs(7))
  print('sqrt', math.sqrt(1e10))
  do local m = math.mod(23e-34, -3) if m == m then print('mod', m) end end
  print('minmax', math.min(6, 86e-35, 443.82888), math.max(644.91717, 1e-45))
  print('frexp', math.frexp(1))
  print('ldexp', math.ldexp(0.75, -13))
  print('pow', 10 ^ 10)
end
-- varargs
do
  local function count(...) return arg.n end
  local function pass(...) return unpack(arg) end
  print(count(), count(nil), count(1, nil), count(nil, nil, 3))
  print(pass(1, 2, 3))
  print((pass(1, 2, 3)))
  local function tail(n, ...) if n == 0 then return arg.n end return tail(n - 1, n, unpack(arg)) end
  print(tail(7))
  local a, b, c = pass(3, 1)
  print(a, b, c)
  local t = {pass(1, 2), pass(3, 4)} print(table.getn(t), t[1], t[2], t[3])
end
-- coerce
do
  print(2^24 + 1)
  print(-1/0 .. "")
  print(0.1 .. "")
  print(16777217 .. "")
  print("0x10" + 0)
  print(1e100 .. "")
  print("  12  " + 0)
  print("5" + "0.25")
  print("5.", tonumber("5."))
  print(".5e1", tonumber(".5e1"))
  print("-7.5", tonumber("-7.5"))
  print("inf", tonumber("inf"))
  print("0x1p4", tonumber("0x1p4"))
  print("-inf", tonumber("-inf"))
  print("1e400", tonumber("1e400"))
  print(19, tonumber("i6d83", 19))
  print(33, tonumber("v395d", 33))
  print(22, tonumber("di3", 22))
  print(16, tonumber("0df90", 16))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
