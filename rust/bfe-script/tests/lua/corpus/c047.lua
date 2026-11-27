-- Generated differential program 047 (generate.py, seed 503).
-- recursion
do
  local function fib(k) if k < 2 then return k end return fib(k - 1) + fib(k - 2) end
  print('fib', fib(14))
  local function down(k) if k == 0 then return 'bottom' end return down(k - 1) end
  print(down(20000))
  local function depth(k) if k == 0 then return 0 end return 1 + depth(k - 1) end
  print(depth(10))
  local function ack(m, k) if m == 0 then return k + 1 elseif k == 0 then return ack(m - 1, 1) end return ack(m - 1, ack(m, k - 1)) end
  print('ack', ack(2, 1))
end
-- trig
do
  print('rad', math.rad(10))
  print('deg', math.deg(0.25))
  print('tan', math.tan(0.5))
  print('log', math.log(0.5))
  print('atan2', math.atan2(-1, 2))
  print('pi', math.pi, math.pi * 2)
end
-- strings
do
  local s = "YZada19b d"
  print(string.sub(s, -4, 13), string.sub(s, -4), string.byte(s, 13))
  print(string.sub(s, -15, 14), string.sub(s, -15), string.byte(s, 14))
  print(string.sub(s, 11, -13), string.sub(s, 11), string.byte(s, -13))
  print(string.sub(s, -7, -7), string.sub(s, -7), string.byte(s, -7))
  print(string.sub(s, -7, -15), string.sub(s, -7), string.byte(s, -15))
  print(string.sub(s, 3, -2), string.sub(s, 3), string.byte(s, -2))
  print(string.sub(s, 9, -10), string.sub(s, 9), string.byte(s, -10))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("-", 0))
  print(string.char(104, 90, 85, 48))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
