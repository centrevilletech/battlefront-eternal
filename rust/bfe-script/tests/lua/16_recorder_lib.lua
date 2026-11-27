-- Synthetic library script of the same archive. It is never loaded; its
-- unguarded call is what tells the recorder that the host provides the name.
function UnusedHelper()
    GuardedHostCall("never_run", 0)
end
