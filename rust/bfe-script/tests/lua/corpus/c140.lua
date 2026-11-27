-- Generated differential program 140 (generate.py, seed 503).
-- recursion
do
  local function fib(k) if k < 2 then return k end return fib(k - 1) + fib(k - 2) end
  print('fib', fib(8))
  local function down(k) if k == 0 then return 'bottom' end return down(k - 1) end
  print(down(5000))
  local function depth(k) if k == 0 then return 0 end return 1 + depth(k - 1) end
  print(depth(1000))
  local function ack(m, k) if m == 0 then return k + 1 elseif k == 0 then return ack(m - 1, 1) end return ack(m - 1, ack(m, k - 1)) end
  print('ack', ack(2, 1))
end
-- globals
do
  gx = 3
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
end
-- compare
do
  print(951 < 719, 951 <= 719, 951 == 719)
  print(2/3 < 78e4, 2/3 <= 78e4, 2/3 == 78e4)
  print(64e17 < 0.546877, 64e17 <= 0.546877, 64e17 == 0.546877)
  print(0.2 < 16777215, 0.2 <= 16777215, 0.2 == 16777215)
  print(8388609 < 257.5346, 8388609 <= 257.5346, 8388609 == 257.5346)
  print("" < "_9BB", "" <= "_9BB", "" == "_9BB")
  print("cB_B" < "aA", "cB_B" <= "aA", "cB_B" == "aA")
  print("" < "BcB9", "" <= "BcB9", "" == "BcB9")
end
-- patterns
do
  print(pcall(string.find, "y)ca", "(b+)(c*)", 1))
  print(pcall(string.find, "1b%ax_1y]1-] ", "(%d+)", 3))
  print(pcall(string.find, "_)%byca]a", "(%d+)", 1))
  print(pcall(string.find, "", "%a+", 20))
  print(pcall(string.find, "bc%", "(%d+)", 20))
  print(pcall(string.find, "[9xx)b-b,-", "[^%s]+", 3))
  print(pcall(string.gsub, " b[ x)9_-cb", "z$", function() return 7 end))
  print(pcall(string.gsub, ")(bc ", "()", "%1-", 0))
  print(pcall(string.gsub, ")c cc zz  .za,x,", "[^%s]+", function() return 7 end, 2))
  do local out = {} for a, b in string.gfind("9", "(%w)(%w)") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
