-- Generated differential program 170 (generate.py, seed 503).
-- arith
do
  do local v = (((76e10) - ((-27.893) - (80e32))) + (3))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = (4294967295)
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = (0.38632983384)
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = (((2^-22) - (374.70533)) * (((9.45860) * (2^-10)) * (-16.476)))
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  print('floor', math.floor(-0.774))
  print('ceil', math.ceil(2^-28))
  print('abs', math.abs(0.1))
  print('sqrt', math.sqrt(2))
  do local m = math.mod(57e24, 2.5) if m == m then print('mod', m) end end
  print('minmax', math.min(8388609, 33554433, 89e27), math.max(3, 658.64767))
  print('frexp', math.frexp(1e-40))
  print('ldexp', math.ldexp(-3, 2))
  print('pow', 10 ^ 0.5)
end
-- closures
do
  local fs = {}
  for i = 1, 2 do fs[i] = function() return i * 3 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(3), counter(10)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
-- patterns
do
  print(pcall(string.find, "1.][][,b(c 1[", "^(%w+)", 1))
  print(pcall(string.find, "0])z", "a?b", 3))
  print(pcall(string.find, ".a(c1", "[^%s]+", 20))
  print(pcall(string.find, "0.9", "(b+)(c*)", -4))
  print(pcall(string.gsub, "1cy %00a%a0 ", "(b+)(c*)", "%%", 0))
  print(pcall(string.gsub, "a]", "%b()", function() return 7 end, 0))
  print(pcall(string.gsub, "b%-,(9", "c-", function() return false end, 1))
  do local out = {} for a, b in string.gfind("y0.a%]_", "%d") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
-- varargs
do
  local function count(...) return arg.n end
  local function pass(...) return unpack(arg) end
  print(count(), count(nil), count(1, nil), count(nil, nil, 3))
  print(pass(1, 2, 3))
  print((pass(1, 2, 3)))
  local function tail(n, ...) if n == 0 then return arg.n end return tail(n - 1, n, unpack(arg)) end
  print(tail(1))
  local a, b, c = pass()
  print(a, b, c)
  local t = {pass(1, 2), pass(3, 4)} print(table.getn(t), t[1], t[2], t[3])
end
