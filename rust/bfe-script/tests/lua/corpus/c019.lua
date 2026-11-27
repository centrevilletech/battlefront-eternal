-- Generated differential program 019 (generate.py, seed 503).
-- coerce
do
  print(2^24 + 1)
  print(123456789 .. "")
  print(-"2")
  print(".5" .. 1)
  print(3 .. "x")
  print(16777217 .. "")
  print(1/0 .. "")
  print(1e15 .. "")
  print("5.", tonumber("5."))
  print("+3", tonumber("+3"))
  print(" 12 ", tonumber(" 12 "))
  print("3x", tonumber("3x"))
  print("1 2", tonumber("1 2"))
  print("-7.5", tonumber("-7.5"))
  print("0x1F", tonumber("0x1F"))
  print(8, tonumber("62 25", 8))
  print(22, tonumber("77b", 22))
  print(22, tonumber("1e dge", 22))
  print(27, tonumber("io", 27))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- recursion
do
  local function fib(k) if k < 2 then return k end return fib(k - 1) + fib(k - 2) end
  print('fib', fib(10))
  local function down(k) if k == 0 then return 'bottom' end return down(k - 1) end
  print(down(20000))
  local function depth(k) if k == 0 then return 0 end return 1 + depth(k - 1) end
  print(depth(10))
  local function ack(m, k) if m == 0 then return k + 1 elseif k == 0 then return ack(m - 1, 1) end return ack(m - 1, ack(m, k - 1)) end
  print('ack', ack(2, 1))
end
-- trig
do
  print('log', math.log(1))
  print('cos', math.cos(3))
  print('deg', math.deg(0.001))
  print('atan', math.atan(0.25))
  print('atan2', math.atan2(0.5, 1))
  print('pi', math.pi, math.pi * 2)
end
