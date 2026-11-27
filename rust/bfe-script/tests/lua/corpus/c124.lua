-- Generated differential program 124 (generate.py, seed 503).
-- recursion
do
  local function fib(k) if k < 2 then return k end return fib(k - 1) + fib(k - 2) end
  print('fib', fib(7))
  local function down(k) if k == 0 then return 'bottom' end return down(k - 1) end
  print(down(5000))
  local function depth(k) if k == 0 then return 0 end return 1 + depth(k - 1) end
  print(depth(10))
  local function ack(m, k) if m == 0 then return k + 1 elseif k == 0 then return ack(m - 1, 1) end return ack(m - 1, ack(m, k - 1)) end
  print('ack', ack(2, 1))
end
-- strings
do
  local s = "b0eXaafXf"
  print(string.sub(s, -5, -5), string.sub(s, -5), string.byte(s, -5))
  print(string.sub(s, 7, -7), string.sub(s, 7), string.byte(s, -7))
  print(string.sub(s, 5, 13), string.sub(s, 5), string.byte(s, 13))
  print(string.sub(s, -14, -5), string.sub(s, -14), string.byte(s, -5))
  print(string.sub(s, 9, 6), string.sub(s, 9), string.byte(s, 6))
  print(string.sub(s, -8, 2), string.sub(s, -8), string.byte(s, 2))
  print(string.sub(s, -4, -7), string.sub(s, -4), string.byte(s, -7))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("", 0))
  print(string.char(38, 93, 98))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- coerce
do
  print(1e15 .. "")
  print(1e100 .. "")
  print("7" * "6")
  print(-0 .. "")
  print("5" + "0.25")
  print(0.1 .. "")
  print(-"2")
  print(" 3e2 " * 1)
  print("0x1F", tonumber("0x1F"))
  print("0x1p4", tonumber("0x1p4"))
  print("5.", tonumber("5."))
  print("3x", tonumber("3x"))
  print("-7.5", tonumber("-7.5"))
  print("0x", tonumber("0x"))
  print("1 2", tonumber("1 2"))
  print(18, tonumber("663", 18))
  print(9, tonumber("05", 9))
  print(21, tonumber("9f", 21))
  print(21, tonumber("083h1", 21))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- loops
do
  do local n, last = 0, nil for i = 1, 3, 2 do n = n + 1 last = i if n > 50 then break end end print('steps', n, last) end
  do local s = 0 for i = 1, 35 do if math.mod(i, 3) == 0 then s = s + i end end print('sum', s) end
  do local k = 0 repeat k = k + 2 until k >= 16 print('repeat', k) end
  do local x = 0.1 local m = 0 while x < 1 do x = x + 0.1 m = m + 1 end print('float loop', m, x) end
  do local t = {} for i = 1, 5 do t[i] = i * i end local s = '' for i, v in ipairs(t) do s = s .. v .. ';' end print(s) end
end
