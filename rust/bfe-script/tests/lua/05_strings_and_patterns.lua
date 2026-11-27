-- String library and Lua 5.0 pattern matching.
print("basic",
    string.len("hello"),
    string.sub("abcdef", 2, 4),
    string.sub("abcdef", -3),
    string.sub("abcdef", 4, 2),
    string.lower("AbC123!"),
    string.upper("AbC123!"),
    string.rep("ab", 3),
    string.rep("ab", 0)
)
print("byte_char",
    string.byte("ABC", 1),
    string.byte("ABC", -1),
    string.byte("ABC", 9),
    string.char(65, 66, 67)
)

print("fmt1", string.format("%d %i %u %x %X %#x %#X %o %#o", -5, 12, 255, 255, 255, 255, 255, 63, 63))
print("fmt2", string.format("%05d %+d % d %-5s %5s %.2s %c %%", 42, 7, 7, "hi", "hi", "hello", 65))
print("fmt3", string.format("%.2f %.3e %.2E %g %G", 1.5, 1.5, 1234.5, 1.5, 1e-5))
print("fmt4", string.format("%q", 'a"b\\c\nd'))

-- Plain and pattern find
local s, e = string.find("hello world", "wor", 1, true)
print("find_plain", s, e)
local s2, e2, c1, c2 = string.find("key=1234;next", "(%a+)=(%d+)")
print("find_pat", s2, e2, c1, c2)
local s3, e3, p1, w, p2 = string.find("foo bar baz", "()(%a+)()", 5)
print("find_pos", s3, e3, p1, w, p2)
local s4, e4 = string.find("a(b(c)d)e", "%b()")
print("find_bal", s4, e4)
local s5, e5 = string.find("abc 123 def", "%f[%d]%d+")
print("find_frontier", s5, e5)
local s6, e6, m = string.find("ab_cd_cd_ef", "(%a+)_%1")
print("find_backref", s6, e6, m)
local s7, e7, minm = string.find("<one><two>", "<(.-)>")
print("find_min", s7, e7, minm)

-- gfind
local words = {}
for w in string.gfind("one, two; three4!", "%a+") do
    table.insert(words, w)
end
print("gfind", table.concat(words, "|"))

-- gsub with string replacement, count limit, and function replacement
local g1, n1 = string.gsub("a=1, b=2, c=3", "(%a+)=(%d+)", "%2:%1")
print("gsub_str", g1, n1)
local g2, n2 = string.gsub("aaa", "a", "b", 2)
print("gsub_lim", g2, n2)
local g3, n3 = string.gsub("10 20 30", "%d+", function(d)
    local n = tonumber(d)
    if n == 20 then return nil end
    return tostring(n + 1)
end)
print("gsub_fn", g3, n3)
