-- Helpers for host API interop, ValueRef pinning across GC, table order, and ScriptRng tests.
function sum_and_diff(a, b)
    return a + b, a - b
end

function install_callback()
    local base = 100
    local cb = function(delta)
        base = base + delta
        return base
    end
    host_register_callback("on_tick", cb)
end

function call_host_lib(x)
    return host_lib.triple(x)
end

function build_ordered_table()
    local t = { "one", "two", "three" }
    t.zeta = 1
    t.alpha = 2
    t.middle = 3
    t[0] = "zero"
    return t
end

function ordered_keys(t)
    local out = {}
    for k, v in pairs(t) do
        table.insert(out, tostring(k) .. "=" .. tostring(v))
    end
    return table.concat(out, ",")
end

function roll_random(seed)
    math.randomseed(seed)
    return math.random(), math.random(10), math.random(5, 15)
end
