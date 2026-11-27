-- Generated differential program 081 (generate.py, seed 503).
-- strings
do
  local s = "YZ 91cYeY"
  print(string.sub(s, 6, 9), string.sub(s, 6), string.byte(s, 9))
  print(string.sub(s, 14, -2), string.sub(s, 14), string.byte(s, -2))
  print(string.sub(s, -4, 8), string.sub(s, -4), string.byte(s, 8))
  print(string.sub(s, 15, -7), string.sub(s, 15), string.byte(s, -7))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("", 4))
  print(string.char(96))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- varargs
do
  local function count(...) return arg.n end
  local function pass(...) return unpack(arg) end
  print(count(), count(nil), count(1, nil), count(nil, nil, 3))
  print(pass(1, 2, 3))
  print((pass(1, 2, 3)))
  local function tail(n, ...) if n == 0 then return arg.n end return tail(n - 1, n, unpack(arg)) end
  print(tail(6))
  local a, b, c = pass()
  print(a, b, c)
  local t = {pass(1, 2), pass(3, 4)} print(table.getn(t), t[1], t[2], t[3])
end
