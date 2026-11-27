-- Fill two library shelves, swap a catalogue card and close the upper shelf.
function ScriptPostLoad()
    assert(AddAIGoal(0, "Hunt", 1) == nil)
    assert(AddAIGoal(8, "Defend", 1, "oak") == nil)
    assert(AddAIGoal(2.5, "Defend", 1, "oak") == nil)
    assert(AddAIGoal(0, "Defend", 0, "oak") == nil)
    assert(AddAIGoal(0, "Follow", 1, "oak") == nil)
    assert(AddAIGoal(0, "Destroy", 1) == nil)
    local cards = {}
    for i = 1, 20 do
        local shelf = 0
        if i > 10 then shelf = 7 end
        cards[i] = AddAIGoal(shelf, "dEfEnD", 0.25 * i, "oak")
        assert(cards[i] ~= nil)
    end
    assert(AddAIGoal(3, "Conquest", 1) == nil)
    DeleteAIGoal(cards[5])
    DeleteAIGoal(cards[5])
    DeleteAIGoal(nil)
    assert(AddAIGoal(3, "CTFOffense", 3.5, string.char(233), "index") > cards[20])
    local closing = CreateTimer("closing")
    SetTimerValue(closing, 0.05)
    OnTimerElapse(function() ClearAIGoals(7) end, closing)
    StartTimer(closing)
end
