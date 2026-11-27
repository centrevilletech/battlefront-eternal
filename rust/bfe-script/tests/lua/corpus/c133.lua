-- Generated differential program 133 (generate.py, seed 503).
-- patterns
do
  print(pcall(string.find, "-9c([c__c", "%f[%w]%w+", 1))
  print(pcall(string.find, " ab9%", "%b()"))
  print(pcall(string.find, "cy_,", "[%a_][%w_]*", 20))
  print(pcall(string.find, "[,,  -)9-0y", "%p", 1))
  print(pcall(string.gsub, ")zz", "[]]", function() return false end, 2))
  print(pcall(string.gsub, "_.xay,[-", "^(%w+)", "#", 2))
  print(pcall(string.gsub, "by]_x])_(%z),y%", "x*", function(c) return string.upper(c) end))
  do local out = {} for a, b in string.gfind("b99(x]1(xyb_1y", "%a+") do table.insert(out, tostring(a) .. ':' .. tostring(b)) end print(table.concat(out, ' ')) end
  print(pcall(string.gsub, 'abc', 'b', {}))
  print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))
end
-- loops
do
  do local n, last = 0, nil for i = 10, 3, 0.25 do n = n + 1 last = i if n > 50 then break end end print('steps', n, last) end
  do local s = 0 for i = 1, 28 do if math.mod(i, 3) == 0 then s = s + i end end print('sum', s) end
  do local k = 0 repeat k = k + 2 until k >= 2 print('repeat', k) end
  do local x = 0.1 local m = 0 while x < 1 do x = x + 0.1 m = m + 1 end print('float loop', m, x) end
  do local t = {} for i = 1, 5 do t[i] = i * i end local s = '' for i, v in ipairs(t) do s = s .. v .. ';' end print(s) end
end
