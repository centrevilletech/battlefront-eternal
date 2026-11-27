-- Generated differential program 072 (generate.py, seed 503).
-- format
do
  print(pcall(string.format, "[%3u]", 2147483647))
  print(pcall(string.format, "[% +9E]", 396))
  print(pcall(string.format, "[%+18E]", 1e-45))
  print(pcall(string.format, "[%0x]", 7))
  print(pcall(string.format, "[% 16.14f]", -25.994))
  print(pcall(string.format, "[%7.4G]", 16777216))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
-- recursion
do
  local function fib(k) if k < 2 then return k end return fib(k - 1) + fib(k - 2) end
  print('fib', fib(9))
  local function down(k) if k == 0 then return 'bottom' end return down(k - 1) end
  print(down(5000))
  local function depth(k) if k == 0 then return 0 end return 1 + depth(k - 1) end
  print(depth(10))
  local function ack(m, k) if m == 0 then return k + 1 elseif k == 0 then return ack(m - 1, 1) end return ack(m - 1, ack(m, k - 1)) end
  print('ack', ack(2, 4))
end
-- strings
do
  local s = "abe "
  print(string.sub(s, 3, -15), string.sub(s, 3), string.byte(s, -15))
  print(string.sub(s, -4, -8), string.sub(s, -4), string.byte(s, -8))
  print(string.sub(s, 15, 9), string.sub(s, 15), string.byte(s, 9))
  print(string.sub(s, 0, -4), string.sub(s, 0), string.byte(s, -4))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("ab", 0))
  print(string.char())
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
