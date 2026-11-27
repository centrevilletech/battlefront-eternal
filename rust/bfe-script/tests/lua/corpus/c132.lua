-- Generated differential program 132 (generate.py, seed 503).
-- coerce
do
  print("1e3" / 4)
  print(0.1 .. "")
  print("7" * "6")
  print("5" + "0.25")
  print(1e15 .. "")
  print(2^24 + 1)
  print("0x10" + 0)
  print(".5" .. 1)
  print("3x", tonumber("3x"))
  print("12", tonumber("12"))
  print("0x", tonumber("0x"))
  print("-7.5", tonumber("-7.5"))
  print(".5e1", tonumber(".5e1"))
  print("0x1p4", tonumber("0x1p4"))
  print("1e400", tonumber("1e400"))
  print(36, tonumber("ux", 36))
  print(35, tonumber("nq", 35))
  print(21, tonumber("28", 21))
  print(20, tonumber("gc0e6", 20))
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
  local a = setmetatable({v = 4}, mt)
  local b = setmetatable({v = 4}, mt)
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
-- trig
do
  print('tan', math.tan(10))
  print('deg', math.deg(0.25))
  print('log', math.log(0.001))
  print('log10', math.log10(2))
  print('atan2', math.atan2(1, 1))
  print('pi', math.pi, math.pi * 2)
end
-- strings
do
  local s = "ZdfYadfbc"
  print(string.sub(s, 10, 14), string.sub(s, 10), string.byte(s, 14))
  print(string.sub(s, 7, -14), string.sub(s, 7), string.byte(s, -14))
  print(string.sub(s, -7, -6), string.sub(s, -7), string.byte(s, -6))
  print(string.sub(s, -13, -13), string.sub(s, -13), string.byte(s, -13))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("", 2))
  print(string.char())
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
