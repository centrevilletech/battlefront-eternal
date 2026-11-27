-- Generated differential program 122 (generate.py, seed 503).
-- patterns
do
  print(pcall(string.find, "az90,% %c_", "[^%s]+", 20))
  print(pcall(string.find, "9", "(%a)(%d)", -4))
  print(pcall(string.find, "c,1-z.[%xz", "%f[%w]%w+", -4))
  print(pcall(string.find, "%9a.z]1", "x*", 20))
  print(pcall(string.find, "(", "^%s*(.-)%s*$", -4))
  print(pcall(string.find, "-100a,,_.),9,", "%s", 1))
  print(pcall(string.find, "-]%(.abx--0,y", "%x+", 1))
  print(pcall(string.gsub, "0", "(%a)(%d)", function() return false end, 1))
  print(pcall(string.gsub, "[x)]ba %%(z9", "z$", function(c) return string.upper(c) end, 1))
  print(pcall(string.gsub, "c,y(90-a", "%f[%w]%w+", "%1-", 1))
  do local out = {} for a, b in string.gfind("", "(%w)(%w)") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
-- trig
do
  print('exp', math.exp(10))
  print('log10', math.log10(10))
  print('rad', math.rad(1))
  print('cos', math.cos(10))
  print('atan2', math.atan2(0.5, -3))
  print('pi', math.pi, math.pi * 2)
end
-- errors
do
  print(pcall(error, 'plain'))
  print(pcall(error, 's'))
  print(pcall(error))
  local function lvl1() error('one', 1) end local function lvl2() error('two', 2) end
  print(pcall(lvl1)) print(pcall(lvl2))
  print(pcall(function() local t = nil return t.x end))
  print(pcall(function() return undefined_global_fn() end))
  print(pcall(function() local t = {} return t.a.b end))
  print(pcall(function() return 1 < 'x' end))
  print(pcall(function() return {} .. 'x' end))
  print(pcall(function() return -{} end))
  print(pcall(function() local s = 'x' return s + 1 end))
  print(pcall(function() local t = setmetatable({}, {__index = function(t, k) error('deep ' .. k) end}) return t.key end))
  print(xpcall(function() error('boom') end, function(m) return 'handled: ' .. m end))
  print(pcall(error, setmetatable({}, {__tostring = function() return 'obj' end})) == false)
  print(pcall(assert, false, 'assert msg'))
  print(pcall(assert, nil))
  print(pcall(assert, 1, 2))
end
