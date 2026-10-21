-- Independent observable contract for the generated controller, with a fresh VM environment
-- at each callback. The adapter enforces Sunrise's durable scalar and timer limits.
local controller = assert(arg[1])
local durable = {foreign = 91}
local timers, next_sequence, generation = {}, 1, "1"
local reloads, callbacks = 0, 0
local fail_write = false
local function clone(value)
    if type(value) ~= "table" then return value end
    local result = {}
    for key, child in pairs(value) do result[key] = clone(child) end
    return result
end
local function same(a, b)
    for key, value in pairs(a) do if b[key] ~= value then return false end end
    for key, value in pairs(b) do if a[key] ~= value then return false end end
    return true
end
local function invoke(handler, event)
    local candidate, pending = clone(durable), clone(timers)
    local sequence = next_sequence
    local writes = 0
    local context = {attempt_generation = generation}
    function context:set_variable(key, value)
        writes = writes + 1
        if fail_write and writes == 2 then error("injected write refusal") end
        assert(type(key) == "string" and #key < 64)
        assert(type(value) == "boolean" or type(value) == "number" or type(value) == "string")
        if type(value) == "string" then assert(#value < 128) end
        candidate[key] = value
        local count = 0
        for _ in pairs(candidate) do count = count + 1 end
        assert(count <= 512)
    end
    function context:clear_variable(key) candidate[key] = nil end
    function context:start_timer(name, delay)
        assert(delay >= 0 and delay <= 0xffffffff)
        local id = tostring(sequence)
        sequence = sequence + 1
        pending[name] = {timer_name = name, timer_sequence = id,
            attempt_generation = generation, delay = delay}
        return id
    end
    function context:cancel_timer(name) pending[name] = nil end
    local state = {}
    function state:variable(key) return candidate[key] end
    local environment = {
        assert=assert,error=error,ipairs=ipairs,tonumber=tonumber,tostring=tostring,
        type=type,select=select,math=math,string=string,table=table,
    }
    local chunk = assert(loadfile(controller, "t", environment))
    local program = chunk()
    local calls = 0
    debug.sethook(function()
        calls = calls + 1000
        assert(calls < 5000000, "callback instruction budget")
    end, "", 1000)
    local ok, message = pcall(program[handler], context, state, event)
    debug.sethook()
    callbacks = callbacks + 1
    if ok then durable, timers, next_sequence = candidate, pending, sequence end
    return ok, message
end
assert(invoke("on_start"))
assert(durable.foreign == 91)
assert(durable["reach_test.g.0"] == 2)
assert(durable["reach_test.g.5"] == 0, "dormant script ran before wake")
local start = clone(durable)
assert(invoke("on_start"))
assert(same(start, durable), "duplicate startup replayed")
for _ = 1, 20 do
    local _, event = next(timers)
    if not event then break end
    assert(invoke("on_load"))
    reloads = reloads + 1
    local before = clone(durable)
    local stale = clone(event)
    stale.attempt_generation = "0"
    assert(invoke("on_event_timer_elapsed", stale))
    assert(same(before, durable), "stale generation advanced execution")
    fail_write = true
    local ok = invoke("on_event_timer_elapsed", event)
    fail_write = false
    assert(not ok and same(before, durable), "refused callback changed durable state")
    assert(invoke("on_event_timer_elapsed", event))
    local committed = clone(durable)
    assert(invoke("on_event_timer_elapsed", event))
    assert(same(committed, durable), "duplicate timer advanced execution")
end
assert(next(timers) == nil, "finished work retained a timer")
assert(durable["reach_test.g.0"] == 4)
assert(durable["reach_test.g.1"] == 7)
assert(durable["reach_test.g.2"] == true)
assert(durable["reach_test.g.3"] == 6)
assert(durable["reach_test.g.4"] == 14)
assert(durable["reach_test.g.5"] == 10)
local finished = clone(durable)
durable["reach_test.program"] = "wrong"
assert(not invoke("on_load"), "changed code resumed old continuations")
durable = clone(finished)
durable["reach_test.vm.1"] = "bad"
assert(not invoke("on_load"), "corrupt continuation was accepted")
durable = clone(finished)
generation = "2"
assert(invoke("on_start"))
assert(durable.foreign == 91 and durable["reach_test.g.0"] == 2)
print(string.format(
    '{"result":14,"branch":7,"ticks":6,"reloads":%d,"callbacks":%d,"duplicate_events_ignored":true,"rollback":true}',
    reloads, callbacks))
