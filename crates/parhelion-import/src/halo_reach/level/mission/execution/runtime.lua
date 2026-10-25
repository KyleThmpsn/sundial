-- State is reconstructed for every callback. The host commits its scalar updates and timer
-- requests together, so a refused callback cannot advance an in-memory continuation.
local prefix = P.namespace .. "."
local timer = prefix .. "tick"
local MAX_BYTES, MAX_CHUNKS, MAX_STEPS = 31750, 250, 10000
local function target(context)
    if P.native then
        assert(context.activity_id == P.native.activity_id, "native mission activity differs")
        assert(context.sdk_build_id == P.native.sdk_build_id, "native mission SDK differs")
    end
end
local function request_value(handle)
    local value = handle and handle.value
    assert(type(value) == "string" and #value > 0 and #value <= 20
        and string.match(value, "^[1-9][0-9]*$"), "invalid native request identity")
    return value
end
local function integer(value)
    assert(type(value) == "number" and value == math.floor(value), "expected mission integer")
    return value
end
local function finite(value)
    assert(type(value) == "number" and value == value and value > -math.huge and value < math.huge,
        "nonfinite mission number")
    return value
end
local function real(value)
    local result = string.unpack("<f", string.pack("<f", finite(value)))
    return finite(result)
end
local function cast(value, kind)
    if kind == 4 then return 0 end
    if kind == 5 then
        if type(value) == "boolean" then return value end
        return finite(value) ~= 0
    end
    if kind == 6 then return real(value) end
    if kind == 7 or kind == 8 then
        finite(value)
        value = value < 0 and math.ceil(value) or math.floor(value)
        local modulus = kind == 7 and 65536 or 4294967296
        value = value % modulus
        if value >= modulus / 2 then value = value - modulus end
        return math.tointeger(value)
    end
    if kind == 9 then assert(type(value) == "string", "expected mission string"); return value end
    if kind == 10 then
        integer(value)
        assert(P.scripts[value], "unbound mission script")
        return value
    end
    error("unsupported mission value type")
end
local function encode(value, parts, depth)
    assert(depth < 64, "mission state nesting")
    local kind = type(value)
    if kind == "boolean" then
        parts[#parts + 1] = value and "t" or "f"
    elseif kind == "number" then
        finite(value)
        parts[#parts + 1] = "d" .. string.pack("<d", value)
    elseif kind == "string" then
        assert(#value <= MAX_BYTES, "mission state string capacity")
        parts[#parts + 1] = "s" .. string.pack("<I4", #value) .. value
    else
        assert(kind == "table" and #value <= 16384, "mission state array capacity")
        parts[#parts + 1] = "a" .. string.pack("<I4", #value)
        for _, child in ipairs(value) do encode(child, parts, depth + 1) end
    end
end
local function decode(bytes)
    local at, nodes = 1, 0
    local function take(count)
        assert(count >= 0 and at + count - 1 <= #bytes, "truncated mission continuation")
        local result = string.sub(bytes, at, at + count - 1)
        at = at + count
        return result
    end
    local function read(depth)
        nodes = nodes + 1
        assert(depth < 64 and nodes <= 16384, "mission continuation capacity")
        local tag = take(1)
        if tag == "t" then return true end
        if tag == "f" then return false end
        if tag == "d" then return finite(string.unpack("<d", take(8))) end
        if tag == "s" then return take(string.unpack("<I4", take(4))) end
        assert(tag == "a", "invalid mission continuation tag")
        local count = string.unpack("<I4", take(4))
        assert(count <= 16384, "mission continuation array capacity")
        local result = {}
        for index = 1, count do result[index] = read(depth + 1) end
        return result
    end
    local result = read(0)
    assert(at == #bytes + 1, "trailing mission continuation bytes")
    return result
end
local function frame(pc, base, parameters, global)
    return {pc, base, parameters, {}, global or 0}
end
local function start(index)
    return {1, 0, {}, {frame(P.scripts[index][4], 0, {})}, {}}
end
local function push(stack, value)
    assert(value ~= nil and #stack < 512, "mission value stack capacity")
    stack[#stack + 1] = value
end
local function pop(stack)
    assert(#stack > 0, "mission value stack underflow")
    local value = stack[#stack]
    stack[#stack] = nil
    return value
end
local function enter(thread, pc, parameters, global)
    assert(#thread[4] < 64, "mission call depth exceeds 64")
    thread[4][#thread[4] + 1] = frame(pc, #thread[3], parameters, global)
end
local function operator(code, values)
    local a, b = values[1], values[2]
    if code == 16 then return a == b end
    if code == 17 then return a ~= b end
    if code == 37 then return not cast(a, 5) end
    for _, value in ipairs(values) do finite(value) end
    if code == 18 then return a > b end
    if code == 19 then return a < b end
    if code == 20 then return a >= b end
    if code == 21 then return a <= b end
    if code == 38 then return real(math.min(math.max(a, b), values[3])) end
    local result = real(a)
    for index = 2, #values do
        local value = real(values[index])
        if code == 9 then result = real(result + value)
        elseif code == 10 then result = real(result - value)
        elseif code == 11 then result = real(result * value)
        elseif code == 12 then assert(value ~= 0, "mission division by zero"); result = real(result / value)
        elseif code == 13 then assert(value ~= 0, "mission remainder by zero"); result = real(math.fmod(result, value))
        elseif code == 14 then result = math.min(result, value)
        elseif code == 15 then result = math.max(result, value)
        else error("unsupported mission operator") end
    end
    return result
end
-- State: logical tick, attempt, threads, global values, initialized flags, next due tick,
-- authoritative timer sequence, terminal failure, completion requested. Thread: status, due tick, value stack,
-- call frames, pending request identity and effect. Handles never enter persisted state.
local function run(thread, state, owner, budget, initializing, context)
    while thread[1] == 1 do
        budget[1] = budget[1] + 1
        assert(budget[1] <= MAX_STEPS, "mission callback instruction capacity")
        local stack, frames = thread[3], thread[4]
        local current = assert(frames[#frames], "missing mission call frame")
        local pc = integer(current[1])
        local instruction = assert(P.code[pc], "invalid mission instruction address")
        current[1] = pc + 1
        local op = instruction[1]
        if op == "push" then push(stack, instruction[2])
        elseif op == "bytes" then
            local parts = {}
            for _, byte in ipairs(instruction[2]) do parts[#parts + 1] = string.char(byte) end
            push(stack, table.concat(parts))
        elseif op == "drop" then pop(stack)
        elseif op == "cast" then push(stack, cast(pop(stack), instruction[2]))
        elseif op == "parameter" then
            local value = current[3][instruction[2]]
            assert(value ~= nil, "missing mission argument")
            push(stack, value)
        elseif op == "global" then
            local index = instruction[2]
            if state[5][index] == 2 then push(stack, state[4][index])
            else
                assert(state[5][index] == 0, "cyclic mission global initializer")
                state[5][index] = 1
                enter(thread, P.globals[index][4], {}, index)
            end
        elseif op == "set" then
            local value = assert(#stack > 0) and stack[#stack]
            state[4][instruction[2]] = value
            state[5][instruction[2]] = 2
        elseif op == "jump" then current[1] = instruction[2]
        elseif op == "false" or op == "true" then
            if cast(pop(stack), 5) == (op == "true") then current[1] = instruction[2] end
        elseif op == "operator" then
            local values = {}
            for index = instruction[3], 1, -1 do values[index] = pop(stack) end
            push(stack, operator(instruction[2], values))
        elseif op == "call" then
            local parameters = {}
            for index = instruction[3], 1, -1 do parameters[index] = pop(stack) end
            enter(thread, P.scripts[instruction[2]][4], parameters, 0)
        elseif op == "return" then
            local value = pop(stack)
            assert(#stack >= current[2], "mission return stack underflow")
            while #stack > current[2] do pop(stack) end
            frames[#frames] = nil
            if current[5] > 0 then
                state[4][current[5]] = value
                state[5][current[5]] = 2
            end
            if #frames > 0 then push(stack, value)
            elseif owner > 0 and P.scripts[owner][3] == 2 then
                thread[1], thread[2] = 2, state[1] + 1
                thread[4] = {frame(P.scripts[owner][4], 0, {})}
            else thread[1] = 4 end
        elseif op == "sleep" then
            assert(not initializing, "global initializer cannot sleep")
            local ticks = integer(pop(stack))
            assert(ticks >= 0 and ticks <= 2147483647, "mission sleep interval")
            push(stack, 0)
            thread[1], thread[2] = 2, state[1] + math.max(1, ticks)
        elseif op == "forever" then
            assert(not initializing, "global initializer cannot suspend")
            push(stack, 0)
            thread[1] = 3
        elseif op == "wait" then
            assert(not initializing, "global initializer cannot wait")
            local condition = cast(pop(stack), 5)
            local wait = current[4]
            if #wait == 0 then wait = {pc, state[1]}; current[4] = wait end
            assert(wait[1] == pc, "mission wait continuation mismatch")
            local expired = instruction[4] >= 0 and state[1] - wait[2] >= instruction[4]
            if condition or expired then
                push(stack, condition)
                current[4] = {}
            else
                local due = state[1] + instruction[3]
                if instruction[4] >= 0 then due = math.min(due, wait[2] + instruction[4]) end
                current[1] = instruction[2]
                thread[1], thread[2] = 2, due
            end
        elseif op == "object" or op == "complete" then
            assert(not initializing and P.native, "unbound native mission request")
            local handle, effect
            if op == "object" then
                local slot = context:slot(instruction[2])
                assert(slot and slot.id == instruction[2] and slot.slot_type == 4,
                    "native mission object is not the bound type-4 slot")
                handle = slot:set_object_active{active=instruction[3]}
                effect = "slot.set_object_active"
            else
                assert(P.native.completion and not context.mission_complete and not state[9],
                    "native mission completion is unbound or already complete")
                handle = context:complete_mission{}
                state[9] = true
                effect = "lifetime.set"
            end
            thread[5] = {request_value(handle), effect}
            thread[1] = 5
            push(stack, 0)
        elseif op == "wake" then
            assert(not initializing, "global initializer cannot wake a script")
            local index = instruction[2]
            local target = state[3][index]
            if target[1] == 0 then state[3][index] = start(index)
            elseif target[1] == 2 or target[1] == 3 then
                for _, call in ipairs(target[4]) do
                    assert(#call[4] == 0, "waking a polling script needs a verified source interrupt binding")
                end
                target[1], target[2] = 1, 0
            end
            push(stack, 0)
        else error("unknown mission instruction") end
    end
end
local function advance(state, budget, context)
    if state[8] ~= "" then return end
    for _, thread in ipairs(state[3]) do
        if thread[1] == 2 and thread[2] <= state[1] then thread[1] = 1 end
    end
    local pending = true
    while pending do
        pending = false
        for index, thread in ipairs(state[3]) do
            if thread[1] == 1 then run(thread, state, index, budget, false, context); pending = true end
        end
    end
end
local function create(context)
    local state = {0, tostring(context.attempt_generation), {}, {}, {}, -1, "", "", false}
    for _ in ipairs(P.scripts) do state[3][#state[3] + 1] = {0, 0, {}, {}, {}} end
    for _, global in ipairs(P.globals) do
        local value = 0
        if global[3] == 5 then value = false end
        state[4][#state[4] + 1] = value
        state[5][#state[5] + 1] = 0
    end
    local budget = {0}
    for index, global in ipairs(P.globals) do
        if state[5][index] == 0 then
            state[5][index] = 1
            run({1, 0, {}, {frame(global[4], 0, {}, index)}, {}}, state, 0, budget, true, context)
        end
    end
    for _, index in ipairs(P.entries) do state[3][index] = start(index) end
    advance(state, budget, context)
    return state
end
local function validate(state, context)
    assert(type(state) == "table" and #state == 9, "invalid mission state")
    assert(type(state[8]) == "string" and #state[8] <= 127, "invalid mission failure")
    assert(type(state[9]) == "boolean", "invalid mission completion state")
    assert(integer(state[1]) >= 0 and state[1] <= 9007199254740991, "mission clock capacity")
    assert(state[2] == tostring(context.attempt_generation), "mission continuation belongs to another attempt")
    assert(#state[3] == #P.scripts and #state[4] == #P.globals and #state[5] == #P.globals,
        "mission continuation table sizes differ")
    for index, value in ipairs(state[4]) do
        cast(value, P.globals[index][3])
        assert(state[5][index] == 2, "mission global was not initialized")
    end
    for _, thread in ipairs(state[3]) do
        assert(type(thread) == "table" and #thread == 5, "invalid mission thread")
        assert(integer(thread[1]) >= 0 and thread[1] <= 5, "invalid mission thread status")
        assert(type(thread[5]) == "table" and (#thread[5] == 0 or #thread[5] == 2),
            "invalid pending mission request")
        assert((thread[1] == 5) == (#thread[5] == 2), "mission pending status differs")
        if thread[1] == 5 then
            request_value{value=thread[5][1]}
            assert(thread[5][2] == "slot.set_object_active" or thread[5][2] == "lifetime.set",
                "invalid pending mission effect")
        end
        assert(integer(thread[2]) >= 0, "invalid mission due tick")
        assert(#thread[3] <= 512 and #thread[4] <= 64, "mission continuation stack capacity")
        assert((thread[1] == 0 or thread[1] == 4) == (#thread[4] == 0),
            "mission thread and frame status differ")
        for _, call in ipairs(thread[4]) do
            assert(#call == 5 and P.code[integer(call[1])], "invalid mission frame address")
            assert(integer(call[2]) >= 0 and call[2] <= #thread[3], "invalid mission frame base")
            assert(#call[3] <= 256 and (#call[4] == 0 or #call[4] == 2), "invalid mission call data")
            assert(integer(call[5]) >= 0 and call[5] <= #P.globals, "invalid mission initializer frame")
        end
    end
    assert(integer(state[6]) >= -1 and type(state[7]) == "string", "invalid mission timer continuation")
end
local function read(context, view)
    target(context)
    assert(view:variable(prefix .. "program") == PROGRAM, "mission program changed")
    local count = integer(view:variable(prefix .. "chunks"))
    assert(count > 0 and count <= MAX_CHUNKS, "mission state chunk capacity")
    local parts = {}
    for index = 1, count do
        local part = view:variable(prefix .. "vm." .. index)
        assert(type(part) == "string" and #part <= 127, "missing mission state chunk")
        parts[index] = part
    end
    local state = decode(table.concat(parts))
    validate(state, context)
    return state
end
local function save(context, view, state)
    local due = -1
    for _, thread in ipairs(state[3]) do
        if state[8] == "" and thread[1] == 2 and (due < 0 or thread[2] < due) then due = thread[2] end
    end
    if due >= 0 and (state[6] ~= due or state[7] == "") then
        local delay = math.ceil((due - state[1]) * 1000 / 30)
        assert(delay >= 0 and delay <= 4294967295, "mission timer delay exceeds host capacity")
        state[7] = tostring(context:start_timer(timer, delay))
    elseif due < 0 then
        context:cancel_timer(timer)
        state[7] = ""
    end
    state[6] = due
    validate(state, context)
    local parts = {}
    encode(state, parts, 0)
    local bytes = table.concat(parts)
    assert(#bytes <= MAX_BYTES, "mission continuation exceeds host variable capacity")
    local count = math.ceil(#bytes / 127)
    local old = view:variable(prefix .. "chunks") or 0
    assert(integer(old) >= 0 and old <= MAX_CHUNKS, "invalid previous mission chunk count")
    for index = 1, count do
        context:set_variable(prefix .. "vm." .. index, string.sub(bytes, (index - 1) * 127 + 1, index * 127))
    end
    for index = count + 1, old do context:clear_variable(prefix .. "vm." .. index) end
    for index, global in ipairs(P.globals) do
        local value = state[4][index]
        assert(type(value) ~= "string" or #value <= 127, "mission global string exceeds host scalar capacity")
        context:set_variable(prefix .. "g." .. global[1], value)
    end
    context:set_variable(prefix .. "chunks", count)
    context:set_variable(prefix .. "program", PROGRAM)
    context:set_variable(prefix .. "attempt", state[2])
    context:set_variable(prefix .. "failure", state[8])
end
return {
    on_start = function(context, view)
        target(context)
        if view:variable(prefix .. "attempt") == tostring(context.attempt_generation) then
            read(context, view)
            return
        end
        assert(view:variable(prefix .. "program") == nil or view:variable(prefix .. "program") == PROGRAM,
            "mission namespace is owned by another program")
        save(context, view, create(context))
    end,
    on_load = function(context, view)
        -- Timers are durable host state too. Loading validates, without replaying any work.
        read(context, view)
    end,
    on_event_timer_elapsed = function(context, view, event)
        if event.timer_name ~= timer or tostring(event.attempt_generation) ~= tostring(context.attempt_generation) then return end
        local state = read(context, view)
        if state[6] < 0 or tostring(event.timer_sequence) ~= state[7] then return end
        state[1], state[6], state[7] = state[6], -1, ""
        advance(state, {0}, context)
        save(context, view, state)
    end,
    on_event_effect_result = function(context, view, event)
        target(context)
        if not P.native or tostring(event.attempt_generation) ~= tostring(context.attempt_generation) then return end
        local state = read(context, view)
        if state[8] ~= "" then return end
        local key = event.request_key and event.request_key.value
        for _, thread in ipairs(state[3]) do
            if thread[1] == 5 and thread[5][1] == key then
                if event.effect ~= thread[5][2] then
                    state[8] = "Native request result has a mismatched effect"
                elseif event.outcome ~= "transport_staged" then
                    local outcome = event.outcome
                    if outcome ~= "refused" and outcome ~= "expired" and outcome ~= "canceled" then outcome = "unknown" end
                    state[8] = "Native request " .. key .. ": " .. outcome
                else
                    thread[1], thread[5] = 1, {}
                    advance(state, {0}, context)
                end
                save(context, view, state)
                return
            end
        end
    end,
}
