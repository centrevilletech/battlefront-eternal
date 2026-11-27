-- Generated differential program 202 (generate.py, seed 503).
-- strings
do
  local s = "XX 99de0b"
  print(string.sub(s, -3, 9), string.sub(s, -3), string.byte(s, 9))
  print(string.sub(s, -6, 0), string.sub(s, -6), string.byte(s, 0))
  print(string.sub(s, 10, 0), string.sub(s, 10), string.byte(s, 0))
  print(string.sub(s, -1, -5), string.sub(s, -1), string.byte(s, -5))
  print(string.sub(s, 4, -13), string.sub(s, 4), string.byte(s, -13))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("", 3))
  print(string.char(109))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- format
do
  print(pcall(string.format, "[% 0u]", 123456))
  print(pcall(string.format, "[% 12e]", 2^-13))
  print(pcall(string.format, "[%q]", "a\"b"))
  print(pcall(string.format, "[%5.2s]", ""))
  print(pcall(string.format, "[%16s]", "hello"))
  print(pcall(string.format, "[%9i]", 7))
  print(pcall(string.format, "[%+2E]", 0.206046))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
-- errors
do
  print(pcall(error, 'plain'))
  print(pcall(error, true))
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
