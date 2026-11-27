-- Generated differential program 230 (generate.py, seed 503).
-- errors
do
  print(pcall(error, 'plain'))
  print(pcall(error, 42))
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
-- format
do
  print(pcall(string.format, "[% +.11f]", 5))
  print(pcall(string.format, "[%d]", 0))
  print(pcall(string.format, "[%0#19f]", 0.8529968))
  print(pcall(string.format, "[% -o]", 123456))
  print(pcall(string.format, "[% 06.9E]", 839.50282))
  print(pcall(string.format, "[%q]", "a\"b"))
  print(pcall(string.format, "[%-d]", 2147483647))
  print(pcall(string.format, "[%q]", "back\\slash"))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
