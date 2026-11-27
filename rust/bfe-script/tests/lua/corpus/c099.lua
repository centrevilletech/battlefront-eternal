-- Generated differential program 099 (generate.py, seed 503).
-- recursion
do
  local function fib(k) if k < 2 then return k end return fib(k - 1) + fib(k - 2) end
  print('fib', fib(17))
  local function down(k) if k == 0 then return 'bottom' end return down(k - 1) end
  print(down(20000))
  local function depth(k) if k == 0 then return 0 end return 1 + depth(k - 1) end
  print(depth(10))
  local function ack(m, k) if m == 0 then return k + 1 elseif k == 0 then return ack(m - 1, 1) end return ack(m - 1, ack(m, k - 1)) end
  print('ack', ack(2, 1))
end
-- varargs
do
  local function count(...) return arg.n end
  local function pass(...) return unpack(arg) end
  print(count(), count(nil), count(1, nil), count(nil, nil, 3))
  print(pass(1, 2, 3))
  print((pass(1, 2, 3)))
  local function tail(n, ...) if n == 0 then return arg.n end return tail(n - 1, n, unpack(arg)) end
  print(tail(1))
  local a, b, c = pass(2, 8, 7)
  print(a, b, c)
  local t = {pass(1, 2), pass(3, 4)} print(table.getn(t), t[1], t[2], t[3])
end
-- strings
do
  local s = "Ydaf d 10Y"
  print(string.sub(s, 0, 8), string.sub(s, 0), string.byte(s, 8))
  print(string.sub(s, -14, 3), string.sub(s, -14), string.byte(s, 3))
  print(string.sub(s, 2, 4), string.sub(s, 2), string.byte(s, 4))
  print(string.sub(s, 12, -14), string.sub(s, 12), string.byte(s, -14))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("ab", 4))
  print(string.char(105, 112, 47, 124))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
