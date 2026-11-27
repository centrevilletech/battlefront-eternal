-- Generated differential program 069 (generate.py, seed 503).
-- globals
do
  gx = 50
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
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
-- patterns
do
  print(pcall(string.find, "]", "%a+", 3))
  print(pcall(string.find, ",)-[]%", "()", 3))
  print(pcall(string.find, "_b9)y)1--xzabb1c]", "[]]", 20))
  print(pcall(string.find, "(-_1 ", "[%.%-]", 1))
  print(pcall(string.gsub, "0 y.", "^%s*(.-)%s*$", function(c) return string.upper(c) end))
  print(pcall(string.gsub, ",", "[%a_][%w_]*", "<%0>", 1))
  print(pcall(string.gsub, "%y)", "[^%s]+", function(c) return string.upper(c) end, 2))
  do local out = {} for a, b in string.gfind("y%,c,]0_-%zx[y", "%d") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
-- format
do
  print(pcall(string.format, "[%11c]", 126))
  print(pcall(string.format, "[% G]", 725.76280))
  print(pcall(string.format, "[%.9g]", 0.063488528))
  print(pcall(string.format, "[% 019.0f]", 67e13))
  print(pcall(string.format, "[% d]", 1))
  print(pcall(string.format, "[%0 g]", 2^-11))
  print(pcall(string.format, "[%+ x]", 255))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
