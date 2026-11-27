-- Generated differential program 142 (generate.py, seed 503).
-- coerce
do
  print(" 3e2 " * 1)
  print("5" + "0.25")
  print(123456789 .. "")
  print(1e15 .. "")
  print(-"2")
  print(1e100 .. "")
  print(-0 .. "")
  print("0x10" + 0)
  print("-inf", tonumber("-inf"))
  print("+3", tonumber("+3"))
  print("0x", tonumber("0x"))
  print(" 12 ", tonumber(" 12 "))
  print("0x1p4", tonumber("0x1p4"))
  print("1e", tonumber("1e"))
  print("12", tonumber("12"))
  print(36, tonumber("4o", 36))
  print(34, tonumber("mjhmn5", 34))
  print(7, tonumber("0 4", 7))
  print(28, tonumber("8cqk", 28))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
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
  local a, b, c = pass(2)
  print(a, b, c)
  local t = {pass(1, 2), pass(3, 4)} print(table.getn(t), t[1], t[2], t[3])
end
-- globals
do
  gx = 70
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
end
