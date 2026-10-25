-- Execute the witnessed real-cache selection. It sleeps one tick and reads a false dialogue
-- flag. No target object or sound binding is supplied by this harness.
local controller, global, delay = arg[1], tonumber(arg[2]), tonumber(arg[3])
local values, timer, sequence = {}, nil, 0
local function invoke(name, event)
    local context = {attempt_generation = "3"}
    local view = {}
    function context:set_variable(key, value) values[key] = value end
    function context:clear_variable(key) values[key] = nil end
    function context:start_timer(key, ms)
        sequence = sequence + 1
        timer = {timer_name=key,timer_sequence=tostring(sequence),attempt_generation="3",delay=ms}
        return tostring(sequence)
    end
    function context:cancel_timer() timer = nil end
    function view:variable(key) return values[key] end
    local environment = {assert=assert,error=error,ipairs=ipairs,tonumber=tonumber,
        tostring=tostring,type=type,math=math,string=string,table=table}
    local program = assert(loadfile(controller, "t", environment))()
    local steps = 0
    debug.sethook(function()
        steps = steps + 1000
        assert(steps < 5000000, "source callback instruction budget")
    end, "", 1000)
    program[name](context, view, event)
    debug.sethook()
end
invoke("on_start")
assert(values["source_test.g." .. global] == false, "source dialogue flag differs")
assert(timer and timer.delay == delay, "source sleep interval differs")
local event = timer
invoke("on_load")
assert(timer == event, "reload replaced durable timer")
invoke("on_event_timer_elapsed", event)
assert(timer == nil, "source script did not complete after its wait")
invoke("on_event_timer_elapsed", event)
invoke("on_load")
assert(values["source_test.g." .. global] == false and timer == nil)
print('{"resumed":true,"global":false}')
