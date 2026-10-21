-- Host contract oracle. Every callback gets a new Lua environment and new handle objects.
local controller = assert(arg[1])
local durable, timers, actions, sequence, generation
local fail_write, activity, sdk, slot_kind, missing, complete
local function clone(v)
    if type(v) ~= "table" then return v end
    local out = {}
    for k, x in pairs(v) do out[k] = clone(x) end
    return out
end
local function same(a, b)
    if type(a) ~= type(b) then return false end
    if type(a) ~= "table" then return a == b end
    for k, v in pairs(a) do if not same(v, b[k]) then return false end end
    for k in pairs(b) do if a[k] == nil then return false end end
    return true
end
local function reset()
    durable, timers, actions, sequence, generation = {foreign=91}, {}, {}, 1, "1"
    fail_write, activity, sdk, slot_kind, missing, complete =
        false, "authored_activity", "sha256:" .. string.rep("a", 64), 4, false, false
end
local function invoke(handler, event)
    local values, pending, queued, next_id = clone(durable), clone(timers), clone(actions), sequence
    local context = {attempt_generation=generation,activity_id=activity,sdk_build_id=sdk,
        mission_complete=complete}
    local function request(effect, active)
        local key = tostring(next_id)
        next_id = next_id + 1
        queued[#queued + 1] = {key=key,effect=effect,active=active}
        return {value=key}
    end
    function context:slot(id)
        assert(id == "authored_object", "source ordinal used as slot")
        if missing then return nil end
        return {id=id,slot_type=slot_kind,set_object_active=function(_, opts)
            assert(type(opts.active) == "boolean")
            return request("slot.set_object_active", opts.active)
        end}
    end
    function context:complete_mission()
        assert(not complete, "duplicate native completion")
        return request("lifetime.set")
    end
    local writes = 0
    function context:set_variable(key, value)
        writes = writes + 1
        if fail_write and writes == 2 then error("injected refusal") end
        assert(#key < 64 and (type(value) ~= "string" or #value < 128))
        values[key] = value
        local count = 0
        for _ in pairs(values) do count = count + 1 end
        assert(count <= 512)
    end
    function context:clear_variable(key) values[key] = nil end
    function context:start_timer(name, delay)
        local key = tostring(next_id)
        next_id = next_id + 1
        pending[name] = {timer_name=name,timer_sequence=key,
            attempt_generation=generation,delay=delay}
        return key
    end
    function context:cancel_timer(name) pending[name] = nil end
    local view = {variable=function(_, key) return values[key] end}
    local environment = {assert=assert,error=error,ipairs=ipairs,tonumber=tonumber,
        tostring=tostring,type=type,select=select,math=math,string=string,table=table}
    local callbacks = assert(loadfile(controller, "t", environment))()
    local ok, message = pcall(callbacks[handler], context, view, event)
    if ok then durable,timers,actions,sequence = values,pending,queued,next_id end
    return ok, message
end
local function result(index, outcome)
    return {attempt_generation=generation,request_key={value=actions[index].key},
        effect=actions[index].effect,outcome=outcome or "transport_staged"}
end
local function progress(value) assert(durable["native_test.g.0"] == value) end
reset()
fail_write = true
assert(not invoke("on_start"))
assert(#actions == 0 and next(timers) == nil and durable.foreign == 91)
fail_write = false
assert(invoke("on_start"))
assert(#actions == 1 and actions[1].active == true)
progress(0)
local started, timer = clone(durable), clone(timers["native_test.tick"])
assert(invoke("on_load"))
assert(invoke("on_start"))
assert(#actions == 1 and same(started, durable), "pending request replayed")
local stale = result(1)
stale.attempt_generation = "0"
assert(invoke("on_event_effect_result", stale))
local unknown = result(1)
unknown.request_key.value = "999"
assert(invoke("on_event_effect_result", unknown))
assert(same(started, durable))
assert(invoke("on_event_effect_result", result(1)))
progress(1)
assert(same(timer, timers["native_test.tick"]), "acknowledgement postponed existing timer")
local committed = clone(durable)
assert(invoke("on_event_effect_result", result(1)))
assert(same(committed, durable), "duplicate result advanced script")
fail_write = true
assert(not invoke("on_event_timer_elapsed", timer))
assert(#actions == 1 and same(committed, durable), "request escaped failed callback")
fail_write = false
assert(invoke("on_event_timer_elapsed", timer))
assert(#actions == 2 and actions[2].active == false)
progress(1)
assert(invoke("on_load"))
assert(invoke("on_event_effect_result", result(2)))
assert(#actions == 3 and actions[3].effect == "lifetime.set")
progress(2)
complete = true
assert(invoke("on_event_effect_result", result(3)))
progress(3)
assert(next(timers) == nil and durable.foreign == 91)
assert(invoke("on_start"))
assert(#actions == 3)
for _, outcome in ipairs({"refused","expired","canceled","unknown"}) do
    reset()
    assert(invoke("on_start"))
    assert(invoke("on_event_effect_result", result(1, outcome)))
    progress(0)
    assert(#durable["native_test.failure"] > 0 and next(timers) == nil)
    assert(invoke("on_load"))
    assert(invoke("on_event_effect_result", result(1)))
    progress(0)
    assert(#actions == 1)
end
reset()
assert(invoke("on_start"))
local wrong = result(1)
wrong.effect = "lifetime.set"
assert(invoke("on_event_effect_result", wrong))
assert(#durable["native_test.failure"] > 0)
for case = 1, 4 do
    reset()
    if case == 1 then sdk = "sha256:" .. string.rep("b", 64)
    elseif case == 2 then activity = "donor"
    elseif case == 3 then slot_kind = 23
    else missing = true end
    assert(not invoke("on_start"), "unsafe target accepted")
    assert(#actions == 0)
end
reset()
assert(invoke("on_start"))
local previous = result(1)
generation = "2"
assert(invoke("on_start"))
assert(#actions == 2)
assert(invoke("on_event_effect_result", previous))
progress(0)
print('{"progress":3,"actions":3,"rollback":true,"reload":true,"terminal_failures":true,"timer_preserved":true}')
