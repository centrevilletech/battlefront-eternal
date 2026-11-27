-- C integer conversions of library arguments. String positions are a C `long`
-- (64 bits on the reference build), counts and most other integers are a C
-- `int`, `%u %o %x %X` take the low 32 bits, and NaN or out-of-range values
-- become the most negative integer of the type.
local nan = 0/0
local function show(...)
  local t = {}
  for i = 1, arg.n do t[i] = tostring(arg[i]) end
  print(arg.n, table.concat(t, " "))
end

-- string positions
show(string.sub("abcdef", 2, 2^32 + 3))
show(string.sub("abcdef", 2^32 + 2))
show(string.sub("abcdef", 2^62))
show(string.sub("abcdef", 2^63))
show(string.sub("abcdef", nan))
show(string.sub("abcdef", 1, nan))
show(string.sub("abcdef", -0.5))
show(string.sub("abcdef", 3.9, 4.9))
show(string.sub("abcdef", -2^40, -3))
show(string.sub("abc", 1, 3e9))
show(string.byte("abcdef", 2^32 + 2))
show(string.byte("abcdef", nan))
show(string.byte("abcdef", -1.5))
show(string.find("abcabc", "b", 2^32 + 1))
show(string.find("abcabc", "c", nan))
show(string.find("abcabc", "a", -2^40))
show(string.find("abcabc", "b", 2^31))
show(string.find("abcabc", "b", 3.99, true))

-- counts and other C int arguments
show(string.rep("ab", 2.9))
show(string.rep("ab", 2^31))
show(string.rep("ab", nan))
show(string.char(65.9, 66.1))
show(pcall(string.char, 256))
show(pcall(string.char, -1))
show(string.gsub("abc", "", "-"))
show(string.gsub("abc", "", "-", 2.9))
show(string.gsub("abc", "", "-", nan))
show(string.gsub("abc", "", "-", -1))
show(math.ldexp(1.5, 3.9))
show(math.ldexp(1, 2^31))
show(pcall(math.random, 0.5))
show(pcall(math.random, 3, 2.5))
show(tonumber("z", 36.9))
show(tonumber("777", 8.5))
show(pcall(tonumber, "10", 1.9))
show(pcall(tonumber, "10", nan))
show(tonumber("ffffffff", 16))
show(tonumber("100000000", 16))
show(pcall(error, "plain", 2^40))
show(getfenv(1.5) == getfenv(1))
show(getfenv(-0.5) == _G)
show(pcall(getfenv, 2^40))

-- tables
local t = {"a", "b", "c", "d"}
show(table.concat(t, ",", 1.9, 3.9))
show(table.concat(t, ",", 2, 2^31))
table.insert(t, 2.7, "x")
show(table.concat(t, ","))
show(table.remove(t, 2.2))
show(table.concat(t, ","))
table.setn(t, 2.5)
show(table.getn(t))

-- string.format integer directives
show(string.format("%d %i", 2^31, -2.9))
show(string.format("%d", nan))
show(string.format("%x %X", 2^32 + 2^20, 2^40))
show(string.format("%u", -1))
show(string.format("%o %x", nan, 2^63))
show(string.format("%c%c", 72.9, 105))
show(string.format("%5d|%-5d|%05d", 42.5, -7.5, 3))
