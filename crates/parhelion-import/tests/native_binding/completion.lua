local path = assert(arg[1])
local values, requests = {}, 0
local context = {attempt_generation="1", activity_id="authored_activity",
    sdk_build_id="sha256:" .. string.rep("a",64), mission_complete=false}
function context:set_variable(k,v) values[k]=v end
function context:clear_variable(k) values[k]=nil end
function context:start_timer() error("unexpected source timer") end
function context:cancel_timer() end
function context:complete_mission()
    requests=requests+1
    return {value="18446744073709550000"}
end
local view = {variable=function(_,k) return values[k] end}
local function invoke(name,event)
    local environment = {assert=assert,error=error,ipairs=ipairs,tonumber=tonumber,
        tostring=tostring,type=type,select=select,math=math,string=string,table=table}
    assert(loadfile(path,"t",environment))()[name](context,view,event)
end
invoke("on_start")
assert(requests==1)
invoke("on_load")
invoke("on_start")
assert(requests==1)
context.mission_complete=true
local result={attempt_generation="1",request_key={value="18446744073709550000"},
    effect="lifetime.set",outcome="transport_staged"}
invoke("on_event_effect_result",result)
assert(values["native_test.failure"]=="")
invoke("on_load")
result.outcome="refused"
invoke("on_event_effect_result",result)
assert(requests==1 and values["native_test.failure"]=="",
    "acknowledged completion remained pending")
print('{"completion_requests":1,"resumed":true}')
