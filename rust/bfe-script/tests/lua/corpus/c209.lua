-- Generated differential program 209 (generate.py, seed 503).
-- recursion
do
  local function fib(k) if k < 2 then return k end return fib(k - 1) + fib(k - 2) end
  print('fib', fib(14))
  local function down(k) if k == 0 then return 'bottom' end return down(k - 1) end
  print(down(100))
  local function depth(k) if k == 0 then return 0 end return 1 + depth(k - 1) end
  print(depth(1000))
  local function ack(m, k) if m == 0 then return k + 1 elseif k == 0 then return ack(m - 1, 1) end return ack(m - 1, ack(m, k - 1)) end
  print('ack', ack(2, 4))
end
-- strings
do
  local s = "bbd"
  print(string.sub(s, 14, -4), string.sub(s, 14), string.byte(s, -4))
  print(string.sub(s, 14, -11), string.sub(s, 14), string.byte(s, -11))
  print(string.sub(s, 13, -14), string.sub(s, 13), string.byte(s, -14))
  print(string.sub(s, 8, -5), string.sub(s, 8), string.byte(s, -5))
  print(string.sub(s, 7, -2), string.sub(s, 7), string.byte(s, -2))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("xyz", 4))
  print(string.char(65, 112, 74))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- loops
do
  do local n, last = 0, nil for i = 0.5, 3, 2 do n = n + 1 last = i if n > 50 then break end end print('steps', n, last) end
  do local s = 0 for i = 1, 36 do if math.mod(i, 3) == 0 then s = s + i end end print('sum', s) end
  do local k = 0 repeat k = k + 2 until k >= 5 print('repeat', k) end
  do local x = 0.1 local m = 0 while x < 1 do x = x + 0.1 m = m + 1 end print('float loop', m, x) end
  do local t = {} for i = 1, 5 do t[i] = i * i end local s = '' for i, v in ipairs(t) do s = s .. v .. ';' end print(s) end
end
