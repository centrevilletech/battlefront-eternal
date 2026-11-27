-- Generated differential program 055 (generate.py, seed 503).
-- varargs
do
  local function count(...) return arg.n end
  local function pass(...) return unpack(arg) end
  print(count(), count(nil), count(1, nil), count(nil, nil, 3))
  print(pass(1, 2, 3))
  print((pass(1, 2, 3)))
  local function tail(n, ...) if n == 0 then return arg.n end return tail(n - 1, n, unpack(arg)) end
  print(tail(3))
  local a, b, c = pass(0, 5, 6, 2)
  print(a, b, c)
  local t = {pass(1, 2), pass(3, 4)} print(table.getn(t), t[1], t[2], t[3])
end
-- coerce
do
  print(123456789 .. "")
  print("0x10" + 0)
  print(2^24 + 1)
  print("7" * "6")
  print(-0 .. "")
  print("  12  " + 0)
  print(1e15 .. "")
  print(-"2")
  print("3x", tonumber("3x"))
  print("+3", tonumber("+3"))
  print("-7.5", tonumber("-7.5"))
  print("0x1F", tonumber("0x1F"))
  print("12", tonumber("12"))
  print("1e400", tonumber("1e400"))
  print("0x", tonumber("0x"))
  print(2, tonumber("0", 2))
  print(28, tonumber("g70", 28))
  print(12, tonumber("7", 12))
  print(4, tonumber("033", 4))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
