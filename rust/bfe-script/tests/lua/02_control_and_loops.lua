-- Control flow, boolean operators (OP_TEST), and all Lua 5.0 loop forms.
local function truth(x)
    if x then return "T" else return "F" end
end
print("truth", truth(nil), truth(false), truth(true), truth(0), truth(""), truth({}))
print("and_or",
    (nil and "a" or "b"),
    (false and "a" or "b"),
    (0 and "a" or "b"),
    ("" and "a" or "b"),
    (1 and 2 and 3),
    (nil or false or 42)
)

-- while & repeat..until
local s = 0
local i = 1
while i <= 5 do
    s = s + i
    i = i + 1
end
local r = 0
repeat
    r = r + 1
until r == 4
print("while_repeat", s, r)

-- numeric for: positive, negative, fractional, string coercion, empty range
local out = {}
for k = 1, 5 do table.insert(out, k) end
for k = 10, 2, -3 do table.insert(out, k) end
for k = 0, 1, 0.5 do table.insert(out, k) end
for k = "1", "3", "1" do table.insert(out, k) end
for k = 5, 1, 1 do table.insert(out, 999) end
print("fornum", table.concat(out, ","))

-- generic for: ipairs
local arr = { "x", "y", "z" }
local ip = {}
for idx, val in ipairs(arr) do
    table.insert(ip, idx .. "=" .. val)
end
print("ipairs", table.concat(ip, ";"))

-- generic for: Lua 5.0 table-shorthand `for k, v in tbl` (OP_TFORPREP)
local t = { 10, 20, 30 }
local sum = 0
for k, v in t do
    sum = sum + k * v
end
print("tforprep", sum)

-- custom stateful iterator
local function range(n)
    local cur = 0
    return function()
        cur = cur + 1
        if cur <= n then return cur, cur * cur end
    end
end
local sq = {}
for k, v in range(4) do
    table.insert(sq, k .. ":" .. v)
end
print("custom_iter", table.concat(sq, ","))
