-- Generated differential program 131 (generate.py, seed 503).
-- patterns
do
  print(pcall(string.find, ")%ya1]y9", "x*", 20))
  print(pcall(string.find, "]-_z99%]a", "%x+", -4))
  print(pcall(string.find, "c,0- cxb99-c(.( ", "^(%w+)", 3))
  print(pcall(string.find, "[[[z 9.%1c ,9%", "[]]", 1))
  print(pcall(string.find, "a].%b z1_xx001", "[]]", 1))
  print(pcall(string.find, "- yx00b_19zz", "[%a_][%w_]*", 3))
  print(pcall(string.gsub, "yy.", "(b+)(c*)", "%%", 1))
  print(pcall(string.gsub, "_a0-.].()%c)y01", "[^%s]+", function(c) return string.upper(c) end, 1))
  print(pcall(string.gsub, ".", "x*", "#"))
  do local out = {} for a, b in string.gfind("]y,)0919c(", "(%w)(%w)") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
-- closures
do
  local fs = {}
  for i = 1, 2 do fs[i] = function() return i * 5 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(9), counter(19)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
-- loops
do
  do local n, last = 0, nil for i = 0.5, 3, 2 do n = n + 1 last = i if n > 50 then break end end print('steps', n, last) end
  do local s = 0 for i = 1, 28 do if math.mod(i, 3) == 0 then s = s + i end end print('sum', s) end
  do local k = 0 repeat k = k + 2 until k >= 11 print('repeat', k) end
  do local x = 0.1 local m = 0 while x < 1 do x = x + 0.1 m = m + 1 end print('float loop', m, x) end
  do local t = {} for i = 1, 5 do t[i] = i * i end local s = '' for i, v in ipairs(t) do s = s .. v .. ';' end print(s) end
end
