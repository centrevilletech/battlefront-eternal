-- Generated differential program 087 (generate.py, seed 503).
-- trig
do
  print('log', math.log(0.5))
  print('deg', math.deg(1))
  print('tan', math.tan(0.25))
  print('cos', math.cos(3))
  print('atan2', math.atan2(0.5, -3))
  print('pi', math.pi, math.pi * 2)
end
-- compare
do
  print(-35.260 < 16777215, -35.260 <= 16777215, -35.260 == 16777215)
  print(-579 < 1, -579 <= 1, -579 == 1)
  print(693.79502 < 2^23, 693.79502 <= 2^23, 693.79502 == 2^23)
  print(-287 < 2147483647, -287 <= 2147483647, -287 == 2147483647)
  print(3.4e38 < 2.13181, 3.4e38 <= 2.13181, 3.4e38 == 2.13181)
  print("bcc" < "cB", "bcc" <= "cB", "bcc" == "cB")
  print("b" < "Ba99", "b" <= "Ba99", "b" == "Ba99")
  print("A00_" < "", "A00_" <= "", "A00_" == "")
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
  local a = setmetatable({v = 3}, mt)
  local b = setmetatable({v = 1}, mt)
  print(a + b, a + 1, 2 + b)
  print(a.nothing, a[1])
  a.new = 'x' print(rawget(a, 'new'))
  print(a == b, a < b, a <= b, a > b, a >= b, a ~= b)
  print(a .. 'x', 'x' .. b, -a, a(5))
  local p = setmetatable({}, {__metatable = 'locked'})
  print(getmetatable(p), pcall(setmetatable, p, {}))
  local base = {greet = function(self) return 'hi ' .. self.name end}
  local obj = setmetatable({name = "ann"}, {__index = base})
  print(obj:greet(), rawget(obj, 'greet'), rawequal(obj, obj))
end
