-- A print shop fills two orders and wins when its press is back in service.
function ScriptPostLoad()
    local ink = "umber_tray"
    local paper = "linen_tray"
    local press = "hand_press"
    local filled = 0

    SpaceAssaultEnable(true)
    SpaceAssaultAddCriticalSystem(paper, 7, 0.625, -0.375, true)
    SpaceAssaultAddCriticalSystem(ink, 11, -0.5, 0.125, true)
    SpaceAssaultLinkCriticalSystems({ink, paper})

    local empty, size, gain = GetObjectShield(press)
    assert(empty == 0 and size == 0 and gain == 0)
    assert(GetObjectShield("paper_rack") == nil)
    assert(GetObjectShield("spare_cartridge") == nil)
    assert(SetProperty(press, "MaxShield", 37))
    assert(SetProperty(press, "CurShield", 19))
    assert(SetProperty(press, "AddShield", 3))
    local charge, capacity, rate = GetObjectShield(press)
    assert(charge == 19 and capacity == 37 and rate == 3)

    assert(not EnableBuildingLockOn(paper, true))
    assert(not SetAIDamageThreshold(press, 0.375))
    assert(not DisableBarriers("press_aisle"))
    assert(not BlockPlanningGraphArcs("shop_entry"))
    assert(not AddDeathRegion("unused_room"))
    assert(not AddLandingRegion("loading_dock"))
    assert(not SetProperty(ink, "IsVisible", false))

    OnObjectDamageName(function()
        SetObjectTeam(press, 0)
        BroadcastVoiceOver("press.jammed", 2)
    end, press)
    local function fill_order(tray, operator)
        AddSpaceAssaultDestroyPoints(operator, tray)
        filled = filled + 1
    end
    OnObjectKillName(fill_order, paper)
    OnObjectKillName(fill_order, ink)
    OnObjectRespawnName(function()
        if filled == 2 and GetObjectTeam(press) == 0 then
            MissionVictory(1)
        end
    end, press)
end
