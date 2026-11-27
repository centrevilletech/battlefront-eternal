-- Generated differential program 172 (generate.py, seed 503).
-- strings
do
  local s = "c0a1aaXY"
  print(string.sub(s, 8, 8), string.sub(s, 8), string.byte(s, 8))
  print(string.sub(s, -13, 10), string.sub(s, -13), string.byte(s, 10))
  print(string.sub(s, -10, 2), string.sub(s, -10), string.byte(s, 2))
  print(string.sub(s, -2, 12), string.sub(s, -2), string.byte(s, 12))
  print(string.sub(s, -4, 7), string.sub(s, -4), string.byte(s, 7))
  print(string.sub(s, 6, 10), string.sub(s, 6), string.byte(s, 10))
  print(string.sub(s, 6, -2), string.sub(s, 6), string.byte(s, -2))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("xyz", 5))
  print(string.char(126, 121, 120, 78))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- coerce
do
  print(10 .. 20)
  print(16777217 .. "")
  print("0x10" + 0)
  print("7" * "6")
  print(2^24 + 1)
  print(-"2")
  print(1e15 .. "")
  print(" 3e2 " * 1)
  print("1e", tonumber("1e"))
  print("-7.5", tonumber("-7.5"))
  print("3x", tonumber("3x"))
  print("-inf", tonumber("-inf"))
  print(" 12 ", tonumber(" 12 "))
  print(".", tonumber("."))
  print("+3", tonumber("+3"))
  print(9, tonumber("231", 9))
  print(9, tonumber("16", 9))
  print(10, tonumber("59129", 10))
  print(30, tonumber("9og", 30))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- meta
do
  local mt = {}
  local function val(x) if type(x) == 'table' then return rawget(x, 'v') end return x end
  mt.__add = function(a, b) return 'add(' .. tostring(val(a)) .. ',' .. tostring(val(b)) .. ')' end
  mt.__index = function(t, k) return 'missing ' .. tostring(k) end
  mt.__newindex = function(t, k, v) rawset(t, k, v .. '!') end
  mt.__eq = function(a, b) return rawget(a, 'v') == rawget(b, 'v') end
  mt.__lt = function(a, b) return rawget(a, 'v') < rawget(b, 'v') end
  mt.__le = function(a, b) return rawget(a, 'v') <= rawget(b, 'v') end
  mt.__concat = function(a, b) return 'cat' end
  mt.__unm = function(a) return 'neg' end
  mt.__call = function(self, x) return 'called ' .. tostring(x) end
  local a = setmetatable({v = 1}, mt)
  local b = setmetatable({v = 1}, mt)
  print(a + b, a + 1, 2 + b)
  print(a.nothing, a[1])
  a.new = 'x' print(rawget(a, 'new'))
  print(a == b, a < b, a <= b, a > b, a >= b, a ~= b)
  print(a .. 'x', 'x' .. b, -a, a(5))
  local p = setmetatable({}, {__metatable = 'locked'})
  print(getmetatable(p), pcall(setmetatable, p, {}))
  local base = {greet = function(self) return 'hi ' .. self.name end}
  local obj = setmetatable({name = "bo"}, {__index = base})
  print(obj:greet(), rawget(obj, 'greet'), rawequal(obj, obj))
end
-- globals
do
  gx = 66
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
end
