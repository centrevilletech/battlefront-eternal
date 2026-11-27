-- Generated differential program 183 (generate.py, seed 503).
-- recursion
do
  local function fib(k) if k < 2 then return k end return fib(k - 1) + fib(k - 2) end
  print('fib', fib(10))
  local function down(k) if k == 0 then return 'bottom' end return down(k - 1) end
  print(down(100))
  local function depth(k) if k == 0 then return 0 end return 1 + depth(k - 1) end
  print(depth(1000))
  local function ack(m, k) if m == 0 then return k + 1 elseif k == 0 then return ack(m - 1, 1) end return ack(m - 1, ack(m, k - 1)) end
  print('ack', ack(2, 1))
end
-- trig
do
  print('sin', math.sin(1))
  print('log10', math.log10(10))
  print('atan', math.atan(0.25))
  print('rad', math.rad(0.25))
  print('atan2', math.atan2(-1, -3))
  print('pi', math.pi, math.pi * 2)
end
