-- Generated differential program 214 (generate.py, seed 503).
-- patterns
do
  print(pcall(string.find, "c%.-._y)0", "%p", 3))
  print(pcall(string.find, "ba-[a]1.z-[)_x", "[^%s]+", 20))
  print(pcall(string.find, "[0", "%a+", 1))
  print(pcall(string.find, "]]", "(%d+)", 3))
  print(pcall(string.find, "c.x0(%0%", "a?b", 1))
  print(pcall(string.find, "_.0)1c)zc% ((x9", "%x+"))
  print(pcall(string.find, "1yc9[b[", "a?b", 3))
  print(pcall(string.gsub, ". ca%_).0(.yy )1", "^%s*(.-)%s*$", "%1-"))
  print(pcall(string.gsub, "-9aa1c", "(%d+)", "#", 0))
  print(pcall(string.gsub, "zy_%99])[)9-0", "a?b", "%1-", 0))
  do local out = {} for a, b in string.gfind(")", "(%w)(%w)") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
-- closures
do
  local fs = {}
  for i = 1, 5 do fs[i] = function() return i * 4 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(3), counter(19)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
