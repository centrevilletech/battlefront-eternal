-- Generated differential program 191 (generate.py, seed 503).
-- arith
do
  do local v = ((86e20) - (0.3))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = (((2^-29) * (2147483647)) + ((31e-36) + ((-1.809) + (2^-6))))
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = ((((79e15) + (0.9605638)) + ((61e-23) + (-875))) * (((0.1) + (605.46212)) / ((782.68475) * (703))))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = (-40.504)
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = (-11.118)
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  do local v = ((4294967295) * ((2^9) - ((2^-8) * (54e3))))
    if v == v then print(5, v, v * 2, -v) else print(5, 'not a number') end end
  do local v = ((((0.55688361687) * (2^26)) * ((94e16) * (839))) - (((2^-15) * (92e7)) * ((19e-12) + (16777216))))
    if v == v then print(6, v, v * 2, -v) else print(6, 'not a number') end end
  do local v = ((((0.7774952528) * (4294967295)) * ((2^-27) - (6))) + (((2^-9) * (6)) / ((2^19) + (2e30))))
    if v == v then print(7, v, v * 2, -v) else print(7, 'not a number') end end
  print('floor', math.floor(37e-14))
  print('ceil', math.ceil(-14.837))
  print('abs', math.abs(9))
  print('sqrt', math.sqrt(16777217))
  do local m = math.mod(0.1, -3) if m == m then print('mod', m) end end
  print('minmax', math.min(2/3, 1, 39e26), math.max(-974, 0.4981735873))
  print('frexp', math.frexp(-5))
  print('ldexp', math.ldexp(0.75, -12))
  print('pow', 10 ^ -2)
end
-- closures
do
  local fs = {}
  for i = 1, 3 do fs[i] = function() return i * 5 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(7), counter(16)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
-- globals
do
  gx = 93
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
end
