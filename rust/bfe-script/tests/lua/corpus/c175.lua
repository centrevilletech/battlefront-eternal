-- Generated differential program 175 (generate.py, seed 503).
-- strings
do
  local s = "X10YZXe"
  print(string.sub(s, 12, 15), string.sub(s, 12), string.byte(s, 15))
  print(string.sub(s, -13, -13), string.sub(s, -13), string.byte(s, -13))
  print(string.sub(s, 3, -4), string.sub(s, 3), string.byte(s, -4))
  print(string.sub(s, -11, 11), string.sub(s, -11), string.byte(s, 11))
  print(string.sub(s, 4, 4), string.sub(s, 4), string.byte(s, 4))
  print(string.sub(s, 11, 6), string.sub(s, 11), string.byte(s, 6))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("-", 5))
  print(string.char(58, 51, 101, 39, 59))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- recursion
do
  local function fib(k) if k < 2 then return k end return fib(k - 1) + fib(k - 2) end
  print('fib', fib(9))
  local function down(k) if k == 0 then return 'bottom' end return down(k - 1) end
  print(down(20000))
  local function depth(k) if k == 0 then return 0 end return 1 + depth(k - 1) end
  print(depth(10))
  local function ack(m, k) if m == 0 then return k + 1 elseif k == 0 then return ack(m - 1, 1) end return ack(m - 1, ack(m, k - 1)) end
  print('ack', ack(2, 1))
end
