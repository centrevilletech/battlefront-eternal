-- Generated differential program 105 (generate.py, seed 503).
-- globals
do
  gx = 1
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
end
-- format
do
  print(pcall(string.format, "[%-+4f]", -27.586))
  print(pcall(string.format, "[%5.2s]", "a b"))
  print(pcall(string.format, "[%5.2s]", ""))
  print(pcall(string.format, "[%f]", 9))
  print(pcall(string.format, "[%14i]", 255))
  print(pcall(string.format, "[%#12i]", 65535))
  print(pcall(string.format, "[%1s]", "tab\9here"))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
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
  local a = setmetatable({v = 0}, mt)
  local b = setmetatable({v = 2}, mt)
  print(a + b, a + 1, 2 + b)
  print(a.nothing, a[1])
  a.new = 'x' print(rawget(a, 'new'))
  print(a == b, a < b, a <= b, a > b, a >= b, a ~= b)
  print(a .. 'x', 'x' .. b, -a, a(5))
  local p = setmetatable({}, {__metatable = 'locked'})
  print(getmetatable(p), pcall(setmetatable, p, {}))
  local base = {greet = function(self) return 'hi ' .. self.name end}
  local obj = setmetatable({name = "cy"}, {__index = base})
  print(obj:greet(), rawget(obj, 'greet'), rawequal(obj, obj))
end
-- strings
do
  local s = "dXbfe aZXXZ"
  print(string.sub(s, -4, 4), string.sub(s, -4), string.byte(s, 4))
  print(string.sub(s, 13, 0), string.sub(s, 13), string.byte(s, 0))
  print(string.sub(s, -12, -15), string.sub(s, -12), string.byte(s, -15))
  print(string.sub(s, 4, 14), string.sub(s, 4), string.byte(s, 14))
  print(string.sub(s, 5, -3), string.sub(s, 5), string.byte(s, -3))
  print(string.sub(s, -7, 12), string.sub(s, -7), string.byte(s, 12))
  print(string.sub(s, -14, 1), string.sub(s, -14), string.byte(s, 1))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("xyz", 1))
  print(string.char(56, 60, 38))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
