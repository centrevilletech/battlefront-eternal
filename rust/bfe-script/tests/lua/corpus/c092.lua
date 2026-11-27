-- Generated differential program 092 (generate.py, seed 503).
-- globals
do
  gx = 41
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
end
-- coerce
do
  print("1e3" / 4)
  print(-"2")
  print(123456789 .. "")
  print("  12  " + 0)
  print(1e100 .. "")
  print(1e15 .. "")
  print("10" + 5)
  print("0x10" + 0)
  print("0x", tonumber("0x"))
  print(" 12 ", tonumber(" 12 "))
  print("-inf", tonumber("-inf"))
  print("  0x10  ", tonumber("  0x10  "))
  print("1e", tonumber("1e"))
  print("-7.5", tonumber("-7.5"))
  print("1 2", tonumber("1 2"))
  print(5, tonumber("1440", 5))
  print(16, tonumber("91", 16))
  print(23, tonumber("0m7i", 23))
  print(28, tonumber("f2p", 28))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- recursion
do
  local function fib(k) if k < 2 then return k end return fib(k - 1) + fib(k - 2) end
  print('fib', fib(17))
  local function down(k) if k == 0 then return 'bottom' end return down(k - 1) end
  print(down(100))
  local function depth(k) if k == 0 then return 0 end return 1 + depth(k - 1) end
  print(depth(300))
  local function ack(m, k) if m == 0 then return k + 1 elseif k == 0 then return ack(m - 1, 1) end return ack(m - 1, ack(m, k - 1)) end
  print('ack', ack(2, 1))
end
-- patterns
do
  print(pcall(string.find, "1] ,c0-y", "z$", 3))
  print(pcall(string.find, "bz)", "a?b", 20))
  print(pcall(string.find, ".91.(", "(a*)", 20))
  print(pcall(string.find, "-aac,].-yya. ", "%a+"))
  print(pcall(string.find, "z]]_0x](9", "%f[%w]%w+", 20))
  print(pcall(string.gsub, "abyx,%a1_)9[", "%x+", "#"))
  print(pcall(string.gsub, "0b-ba)", "a?b", "%1-", 2))
  print(pcall(string.gsub, "c[)9]x1c)%-c", "(%a)(%d)", function() return false end, 2))
  do local out = {} for a, b in string.gfind("[", "[,.]") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
