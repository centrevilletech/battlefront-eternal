-- Helpers for instruction budget, call depth, memory limit, and cyclic GC tests.
function infinite_loop()
    while true do
    end
end

function infinite_loop_pcall()
    return pcall(infinite_loop)
end

function deep_recurse(n)
    return 1 + deep_recurse(n + 1)
end

function deep_recurse_pcall()
    return pcall(deep_recurse, 0)
end

function memory_bomb()
    local s = "abcdefghij"
    while true do
        s = s .. s
    end
end

function memory_bomb_pcall()
    return pcall(memory_bomb)
end

function cycle_churn(n)
    local total = 0
    for i = 1, n do
        local a = { id = i }
        local b = { back = a }
        a.next = b
        a.fn = function() return a.id + b.back.id end
        total = total + a.fn()
    end
    return total
end
