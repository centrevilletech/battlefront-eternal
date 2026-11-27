-- Generated differential program 226 (generate.py, seed 503).
-- recursion
do
  local function fib(k) if k < 2 then return k end return fib(k - 1) + fib(k - 2) end
  print('fib', fib(10))
  local function down(k) if k == 0 then return 'bottom' end return down(k - 1) end
  print(down(20000))
  local function depth(k) if k == 0 then return 0 end return 1 + depth(k - 1) end
  print(depth(300))
  local function ack(m, k) if m == 0 then return k + 1 elseif k == 0 then return ack(m - 1, 1) end return ack(m - 1, ack(m, k - 1)) end
  print('ack', ack(2, 2))
end
-- trig
do
  print('tan', math.tan(0.5))
  print('cos', math.cos(10))
  print('exp', math.exp(10))
  print('rad', math.rad(1))
  print('atan2', math.atan2(1, -3))
  print('pi', math.pi, math.pi * 2)
end
