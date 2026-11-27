-- Generated differential program 173 (generate.py, seed 503).
-- closures
do
  local fs = {}
  for i = 1, 5 do fs[i] = function() return i * 7 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(7), counter(15)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
-- trig
do
  print('log10', math.log10(0.001))
  print('deg', math.deg(0.25))
  print('sin', math.sin(0.5))
  print('rad', math.rad(2))
  print('atan2', math.atan2(-1, -3))
  print('pi', math.pi, math.pi * 2)
end
