-- Tables, SETLIST flush (> 32 items), table library, OOP (OP_SELF), and all Lua 5.0 metamethods.

-- More than LFIELDS_PER_FLUSH (32) literal elements
local big = {
     1,  2,  3,  4,  5,  6,  7,  8,  9, 10,
    11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
    21, 22, 23, 24, 25, 26, 27, 28, 29, 30,
    31, 32, 33, 34, 35, 36, 37, 38, 39, 40
}
print("big", table.getn(big), big[1], big[32], big[33], big[40])

-- table.insert / remove / setn / getn / concat / foreachi / sort
local t = { 5, 1, 4, 2, 3 }
table.insert(t, 6)
table.insert(t, 1, 9)
local rem = table.remove(t, 2)
table.sort(t)
print("sorted", rem, table.getn(t), table.concat(t, ","))
table.sort(t, function(a, b) return a > b end)
print("revsorted", table.concat(t, ","))

local t2 = { 10, 20, 30 }
table.setn(t2, 2)
print("setn", table.getn(t2), rawget(t2, "n"))
local t3 = { 10, 20, n = 5 }
print("field_n", table.getn(t3))
table.setn(t3, 3)
print("field_n_after", table.getn(t3), t3.n)

local fi_res = table.foreachi({ "a", "b", "c" }, function(i, v)
    if i == 2 then return v .. "!" end
end)
print("foreachi", fi_res)

-- Delete during pairs traversal
local del = { a = 1, b = 2, c = 3, d = 4 }
local seen = 0
for k, v in pairs(del) do
    seen = seen + v
    if v == 2 or v == 4 then
        del[k] = nil
    end
end
local left = 0
for k, v in pairs(del) do
    left = left + v
end
print("delete_iter", seen, left)

-- Metatables: OOP + __index (table & fn) + __newindex (table & fn)
local Base = { kind = "base" }
Base.__index = Base
function Base:describe(tag)
    return tag .. ":" .. self.kind .. ":" .. self.name
end
local obj = setmetatable({ name = "hero" }, Base)
print("oop", obj:describe("unit"))

local proxy_store = {}
local proxy = setmetatable({}, {
    __index = function(self, k) return "get_" .. tostring(k) end,
    __newindex = function(self, k, v) proxy_store[k] = v * 2 end,
})
proxy.x = 21
print("proxy", proxy.foo, proxy_store.x)

-- Arithmetic, comparison, concat, call, tostring, __metatable protection
local Vec = {}
Vec.__index = Vec
function Vec.new(x) return setmetatable({ x = x }, Vec) end
function Vec.__add(a, b) return Vec.new(a.x + b.x) end
function Vec.__sub(a, b) return Vec.new(a.x - b.x) end
function Vec.__mul(a, b) return Vec.new(a.x * b.x) end
function Vec.__div(a, b) return Vec.new(a.x / b.x) end
function Vec.__pow(a, b) return Vec.new(a.x ^ b.x) end
function Vec.__unm(a) return Vec.new(-a.x) end
function Vec.__eq(a, b) return a.x == b.x end
function Vec.__lt(a, b) return a.x < b.x end
function Vec.__concat(a, b) return tostring(a) .. "+" .. tostring(b) end
function Vec.__call(a, k) return a.x * k end
function Vec.__tostring(a) return "V(" .. a.x .. ")" end

local v1, v2 = Vec.new(6), Vec.new(2)
print("mt_arith", tostring(v1 + v2), tostring(v1 - v2), tostring(v1 * v2), tostring(v1 / v2), tostring(v1 ^ v2), tostring(-v1))
print("mt_cmp", v1 == Vec.new(6), rawequal(v1, Vec.new(6)), v2 < v1, v2 <= v1, v1 <= Vec.new(6), v1 <= v2)
print("mt_other", v1 .. v2, v1(5))

local prot = setmetatable({}, { __metatable = "locked" })
print("mt_prot", getmetatable(prot))
local ok_prot = pcall(setmetatable, prot, {})
print("mt_prot_set", ok_prot)
