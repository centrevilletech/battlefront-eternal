-- Closures, upvalue sharing/closing, Lua 5.0 varargs (`arg`), multi-returns, and tail calls.

-- Shared mutable upvalue
local function make_counter(start)
    local n = start
    local function inc(d)
        n = n + d
        return n
    end
    local function get()
        return n
    end
    return inc, get
end
local inc, get = make_counter(10)
print("counter", inc(5), inc(3), get())

-- Closing loop locals (OP_CLOSE)
local fns = {}
for i = 1, 4 do
    local x = i * 10
    fns[i] = function() return x end
end
print("loop_close", fns[1](), fns[2](), fns[3](), fns[4]())

-- Nested upvalues (grandchild capturing grandparent via OP_GETUPVAL in OP_CLOSURE)
local function outer(a)
    local function mid(b)
        return function(c)
            return a + b + c
        end
    end
    return mid
end
print("nested_upval", outer(1)(20)(300))

-- Lua 5.0 varargs via `arg` table
local function vsum(prefix, ...)
    local total = 0
    for i = 1, arg.n do
        if arg[i] then
            total = total + arg[i]
        end
    end
    return prefix, arg.n, total
end
print("varargs1", vsum("a", 1, 2, 3, 4))
print("varargs2", vsum("b"))
print("varargs3", vsum("c", 10, nil, 20))

-- Multiple returns, open calls, and SETLISTO
local function multi()
    return 10, 20, 30
end
local function wrap()
    return multi()
end
local a, b, c, d = wrap()
print("multi", a, b, c, d)
local list = { 1, 2, multi() }
print("setlisto", table.getn(list), table.concat(list, ","))
local trimmed = { (multi()), 99 }
print("trimmed", table.getn(trimmed), table.concat(trimmed, ","))

-- Proper tail recursion (depth 2000 exceeds normal call stack limit of 200)
local function tail_sum(n, acc)
    if n == 0 then
        return acc
    end
    return tail_sum(n - 1, acc + n)
end
print("tailcall", tail_sum(2000, 0))
