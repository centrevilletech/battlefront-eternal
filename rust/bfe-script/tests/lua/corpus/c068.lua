-- Generated differential program 068 (generate.py, seed 503).
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
-- strings
do
  local s = "bfZYYe be"
  print(string.sub(s, 10, -3), string.sub(s, 10), string.byte(s, -3))
  print(string.sub(s, -15, 1), string.sub(s, -15), string.byte(s, 1))
  print(string.sub(s, -14, -6), string.sub(s, -14), string.byte(s, -6))
  print(string.sub(s, -4, 10), string.sub(s, -4), string.byte(s, 10))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("", 1))
  print(string.char())
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- patterns
do
  print(pcall(string.find, ",0)(a[1]a0%-", "(.)%1", -4))
  print(pcall(string.find, "y(b)_", "z$", 20))
  print(pcall(string.find, "]-%0ca", "%x+", 1))
  print(pcall(string.find, "0-9 -[0(1c_", "[%.%-]"))
  print(pcall(string.gsub, "0(y(1z]_xcb%ca-", "^(%w+)", "<%0>"))
  print(pcall(string.gsub, " ([-b-.-%,", ".-b", "<%0>", 0))
  print(pcall(string.gsub, "", "c-", "%%"))
  do local out = {} for a, b in string.gfind(" b9)c_b)", "%d") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
