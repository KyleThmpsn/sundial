local missions = require("missions")
local mission = require(assert(missions[ACTIVITY], "converted mission is absent from the SDK"))
local key = "reach.encounter."
for _, phase in ipairs(PHASES) do
    for _, gate in ipairs(phase.gates) do
        local found
        for _, alias in ipairs(gate.aliases) do
            local slot = mission.slots[alias]
            if slot and slot.id == gate.id then found = slot end
        end
        assert(found, "mission gate is absent from the SDK")
        assert(found.type == 31 and found.index == gate.slot, "mission gate has the wrong slot")
    end
    for _, variant in ipairs(phase.variants) do
        for _, placement in ipairs(variant.placements) do
            local alias = placement.qualified
            if mission.Squad[alias] == nil then alias = placement.name end
            local slot = assert(mission.slots[alias], "encounter squad has no SDK slot")
            assert(string.sub(slot.id, 1, #placement.object) == placement.object, "encounter squad belongs to another object")
            placement.id = assert(mission.Squad[alias], "encounter squad is absent")
        end
    end
end

-- Writes are commands committed after a callback. This overlay makes the callback's own
-- transitions visible without depending on mutable host snapshots.
local function variables(context, state)
    local changed = {}
    local function get(name)
        if changed[name] ~= nil then return changed[name][1] end
        return state:variable(key .. name)
    end
    local function put(name, value)
        changed[name] = {value}
        if value == nil then context:clear_variable(key .. name)
        else context:set_variable(key .. name, value) end
    end
    return get, put
end

local function prefix(index) return "phase." .. index .. "." end
local function variant_for(phase, region)
    for index, variant in ipairs(phase.variants) do
        if variant.region == region then return variant, index end
    end
end

local function pump(context, get, put)
    if get("failure") ~= nil then return end
    local index = get("next")
    local phase = PHASES[index]
    if not phase then return end
    local field = prefix(index)
    if get(field .. "queued") then return end
    local region = get("region")
    local variant, variant_index = variant_for(phase, region)
    if not variant or (index == 1 and region ~= 0) then return end
    local activated = index == 1
    for gate_index, gate in ipairs(phase.gates) do
        local gate_field = field .. "gate." .. gate_index .. "."
        if gate.region == region then
            if get(gate_field .. "ready") and get(gate_field .. "seen") then
                activated = true
            elseif not get(gate_field .. "request") and not get(gate_field .. "ready") then
                local request = context:slot(gate.id):fire_trigger{}
                put(gate_field .. "request", request.value)
            end
        end
    end
    if not activated then return end
    for placement_index, placement in ipairs(variant.placements) do
        local squad = context:squad(placement.id)
        local counts = squad:counts()
        for member = 1, counts.count do counts:set(member, 0) end
        counts:set(1, placement.count)
        local request = squad:place{counts = counts}
        put(field .. "request." .. placement_index, request.value)
    end
    put(field .. "variant", variant_index)
    put(field .. "queued", true)
    for gate_index, gate in ipairs(phase.gates) do
        if gate.region == region then
            local request = context:slot(gate.id):disarm_trigger{}
            put(field .. "cleanup." .. gate_index, request.value)
        end
    end
end

local function initialize(context, state)
    local get, put = variables(context, state)
    local attempt = tostring(context.attempt_generation)
    if get("attempt") == attempt then return end
    put("attempt", attempt)
    put("next", 1)
    put("region", nil)
    put("generation", nil)
    put("failure", nil)
    for index, phase in ipairs(PHASES) do
        local field = prefix(index)
        put(field .. "queued", nil)
        put(field .. "variant", nil)
        local count = 0
        for _, variant in ipairs(phase.variants) do count = math.max(count, #variant.placements) end
        for placement = 1, count do
            put(field .. "request." .. placement, nil)
            put(field .. "staged." .. placement, nil)
        end
        for gate in ipairs(phase.gates) do
            for _, suffix in ipairs({"request", "ready", "seen"}) do
                put(field .. "gate." .. gate .. "." .. suffix, nil)
            end
            put(field .. "cleanup." .. gate, nil)
        end
    end
end

local function current(context, state, event)
    return tostring(event.attempt_generation) == tostring(context.attempt_generation)
        and state:variable(key .. "attempt") == tostring(context.attempt_generation)
end

return {
    on_start = initialize,
    on_load = function(context, state)
        assert(state:variable(key .. "attempt") == tostring(context.attempt_generation), "encounter belongs to another attempt")
    end,
    on_event_client_state_changed = function(context, state, event)
        if not current(context, state, event) then return end
        local get, put = variables(context, state)
        local region = event.entered == true and event.held_region_index or nil
        local generation = tostring(event.source_generation)
        if region ~= get("region") or generation ~= get("generation") then
            local index = get("next")
            local phase = PHASES[index]
            if phase then
                for gate in ipairs(phase.gates) do
                    for _, suffix in ipairs({"request", "ready", "seen"}) do
                        put(prefix(index) .. "gate." .. gate .. "." .. suffix, nil)
                    end
                end
            end
        end
        put("region", region)
        put("generation", generation)
        pump(context, get, put)
    end,
    on_event_player_trigger = function(context, state, event)
        if not current(context, state, event) then return end
        local get, put = variables(context, state)
        if tostring(event.source_generation) ~= get("generation") then return end
        local index = get("next")
        local phase = PHASES[index]
        if not phase or get(prefix(index) .. "queued") then return end
        for gate_index, gate in ipairs(phase.gates) do
            local field = prefix(index) .. "gate." .. gate_index .. "."
            if gate.region == get("region") and event.slot_type == 31
                and event.object_tag == gate.object and event.registry_key == gate.key
                and event.slot_index == gate.slot and event.volume_registry_key == gate.key
                and event.volume_slot_type == 60 and event.volume_slot_index == gate.volume
                and (get(field .. "ready") or get(field .. "request")) then
                put(field .. "seen", true)
                pump(context, get, put)
                return
            end
        end
    end,
    on_event_effect_result = function(context, state, event)
        if not current(context, state, event) then return end
        local get, put = variables(context, state)
        local request = event.request_key and event.request_key.value
        if not request then return end
        for index, phase in ipairs(PHASES) do
            local field = prefix(index)
            local variant = phase.variants[get(field .. "variant")]
            local function result(name, expected)
                if get(name) ~= request then return false end
                put(name, nil)
                if event.effect ~= expected or event.outcome ~= "transport_staged" then
                    put("failure", "Encounter request " .. tostring(event.outcome))
                end
                return true
            end
            for gate in ipairs(phase.gates) do
                if result(field .. "cleanup." .. gate, "slot.fire_trigger") then return end
                local gate_field = field .. "gate." .. gate .. "."
                if result(gate_field .. "request", "slot.fire_trigger") then
                    if event.outcome == "transport_staged" then put(gate_field .. "ready", true) end
                    pump(context, get, put)
                    return
                end
            end
            if variant then
                for placement in ipairs(variant.placements) do
                    if result(field .. "request." .. placement, "squad.place") then
                        if event.outcome == "transport_staged" then put(field .. "staged." .. placement, true) end
                        local all = get("failure") == nil
                        for member in ipairs(variant.placements) do all = all and get(field .. "staged." .. member) == true end
                        if all and get("next") == index then put("next", index + 1) end
                        pump(context, get, put)
                        return
                    end
                end
            end
        end
    end,
}
