-- Generated differential program 074 (generate.py, seed 503).
-- recursion
do
  local function fib(k) if k < 2 then return k end return fib(k - 1) + fib(k - 2) end
  print('fib', fib(8))
  local function down(k) if k == 0 then return 'bottom' end return down(k - 1) end
  print(down(100))
  local function depth(k) if k == 0 then return 0 end return 1 + depth(k - 1) end
  print(depth(2500))
  local function ack(m, k) if m == 0 then return k + 1 elseif k == 0 then return ack(m - 1, 1) end return ack(m - 1, ack(m, k - 1)) end
  print('ack', ack(2, 2))
end
-- varargs
do
  local function count(...) return arg.n end
  local function pass(...) return unpack(arg) end
  print(count(), count(nil), count(1, nil), count(nil, nil, 3))
  print(pass(1, 2, 3))
  print((pass(1, 2, 3)))
  local function tail(n, ...) if n == 0 then return arg.n end return tail(n - 1, n, unpack(arg)) end
  print(tail(6))
  local a, b, c = pass(4, 8, 8)
  print(a, b, c)
  local t = {pass(1, 2), pass(3, 4)} print(table.getn(t), t[1], t[2], t[3])
end
-- loops
do
  do local n, last = 0, nil for i = 1, 1, 2 do n = n + 1 last = i if n > 50 then break end end print('steps', n, last) end
  do local s = 0 for i = 1, 37 do if math.mod(i, 3) == 0 then s = s + i end end print('sum', s) end
  do local k = 0 repeat k = k + 2 until k >= 15 print('repeat', k) end
  do local x = 0.1 local m = 0 while x < 1 do x = x + 0.1 m = m + 1 end print('float loop', m, x) end
  do local t = {} for i = 1, 5 do t[i] = i * i end local s = '' for i, v in ipairs(t) do s = s .. v .. ';' end print(s) end
end
-- patterns
do
  print(pcall(string.find, ")b(a)%)-.[bx", "%f[%w]%w+"))
  print(pcall(string.find, "911([_1%)9]]))([", "%x+", 20))
  print(pcall(string.find, "b-z", "%p", 20))
  print(pcall(string.find, "00)9z", "x*", 20))
  print(pcall(string.find, "x]z1]y(z[1z1,(y", "(%d+)", -4))
  print(pcall(string.find, "(-y0_19_x", "(%d+)", -4))
  print(pcall(string.gsub, "09(", "%s", "%%", 2))
  print(pcall(string.gsub, "b1ya__", "%s", function() return false end, 0))
  print(pcall(string.gsub, "9%_y1(__", "z$", function() return 7 end, 1))
  do local out = {} for a, b in string.gfind("", "%d") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
