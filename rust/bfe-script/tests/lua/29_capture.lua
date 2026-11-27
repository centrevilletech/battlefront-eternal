local scale = 13
return function(a, b, c, d, e)
    return function() return a + c + scale * b + e - d end
end
