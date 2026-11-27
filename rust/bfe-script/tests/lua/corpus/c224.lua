-- Generated differential program 224 (generate.py, seed 503).
-- strings
do
  local s = "eecc"
  print(string.sub(s, -14, 2), string.sub(s, -14), string.byte(s, 2))
  print(string.sub(s, 4, -14), string.sub(s, 4), string.byte(s, -14))
  print(string.sub(s, -1, -7), string.sub(s, -1), string.byte(s, -7))
  print(string.sub(s, -14, -14), string.sub(s, -14), string.byte(s, -14))
  print(string.sub(s, 1, 13), string.sub(s, 1), string.byte(s, 13))
  print(string.sub(s, -8, 11), string.sub(s, -8), string.byte(s, 11))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("", 1))
  print(string.char(54))
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
  print(tail(4))
  local a, b, c = pass(5, 8, 8, 3)
  print(a, b, c)
  local t = {pass(1, 2), pass(3, 4)} print(table.getn(t), t[1], t[2], t[3])
end
