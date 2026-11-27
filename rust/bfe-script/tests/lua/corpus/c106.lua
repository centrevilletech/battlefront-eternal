-- Generated differential program 106 (generate.py, seed 503).
-- closures
do
  local fs = {}
  for i = 1, 4 do fs[i] = function() return i * 8 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(6), counter(13)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
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
  local s = "X  Y"
  print(string.sub(s, 13, -12), string.sub(s, 13), string.byte(s, -12))
  print(string.sub(s, -4, -1), string.sub(s, -4), string.byte(s, -1))
  print(string.sub(s, 15, -4), string.sub(s, 15), string.byte(s, -4))
  print(string.sub(s, -4, 10), string.sub(s, -4), string.byte(s, 10))
  print(string.sub(s, 12, 8), string.sub(s, 12), string.byte(s, 8))
  print(string.sub(s, -11, -2), string.sub(s, -11), string.byte(s, -2))
  print(string.sub(s, 2, 12), string.sub(s, 2), string.byte(s, 12))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("xyz", 3))
  print(string.char(91))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- arith
do
  do local v = (-35.959)
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = (2147483647)
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = ((4294967295) * ((196.99847) + (16777216)))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = ((64e-32) * ((4294967295) - ((2^0) * (16777216))))
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  print('floor', math.floor(903.98065))
  print('ceil', math.ceil(965.97716))
  print('abs', math.abs(994.68272))
  print('sqrt', math.sqrt(0.5))
  do local m = math.mod(5, 0.75) if m == m then print('mod', m) end end
  print('minmax', math.min(16777216, 4294967295, -0.886), math.max(497.60871, 67e10))
  print('frexp', math.frexp(-5))
  print('ldexp', math.ldexp(1, 10))
  print('pow', 10 ^ 0.5)
end
