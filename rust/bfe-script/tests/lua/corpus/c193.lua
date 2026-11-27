-- Generated differential program 193 (generate.py, seed 503).
-- format
do
  print(pcall(string.format, "[%+15E]", 3.4e38))
  print(pcall(string.format, "[%019X]", 65535))
  print(pcall(string.format, "[%- i]", 65535))
  print(pcall(string.format, "[%5.2s]", "a b"))
  print(pcall(string.format, "[%0#15d]", -1))
  print(pcall(string.format, "[%s]", "hello"))
  print(pcall(string.format, "[%0 11e]", 2^-7))
  print(pcall(string.format, "[%-+12X]", 255))
  print(pcall(string.format, "[%-#x]", 0))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
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
