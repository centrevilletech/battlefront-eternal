-- Generated differential program 065 (generate.py, seed 503).
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
-- format
do
  print(pcall(string.format, "[%# 18.7E]", -41.228))
  print(pcall(string.format, "[%s]", "xxxxxxxxxxxx"))
  print(pcall(string.format, "[%019.12G]", 971.5552))
  print(pcall(string.format, "[%-12u]", -98765))
  print(pcall(string.format, "[%s]", "hello"))
  print(pcall(string.format, "[%-9.8f]", -795))
  print(pcall(string.format, "[%18.8s]", "tab\9here"))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
-- compare
do
  print(740.81626 < 16777215, 740.81626 <= 16777215, 740.81626 == 16777215)
  print(0.967876 < 16777216, 0.967876 <= 16777216, 0.967876 == 16777216)
  print(848.31632 < 2^-30, 848.31632 <= 2^-30, 848.31632 == 2^-30)
  print(2^3 < 959, 2^3 <= 959, 2^3 == 959)
  print("a_a" < "B", "a_a" <= "B", "a_a" == "B")
  print("_Bcc" < "A", "_Bcc" <= "A", "_Bcc" == "A")
  print("ccBa" < "9_0B", "ccBa" <= "9_0B", "ccBa" == "9_0B")
end
-- strings
do
  local s = "ba 9bXZaYaa"
  print(string.sub(s, -11, 4), string.sub(s, -11), string.byte(s, 4))
  print(string.sub(s, -9, 9), string.sub(s, -9), string.byte(s, 9))
  print(string.sub(s, -1, -3), string.sub(s, -1), string.byte(s, -3))
  print(string.sub(s, 14, -7), string.sub(s, 14), string.byte(s, -7))
  print(string.sub(s, -11, 10), string.sub(s, -11), string.byte(s, 10))
  print(string.sub(s, -14, -4), string.sub(s, -14), string.byte(s, -4))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("ab", 4))
  print(string.char(79))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
