-- lua5.1 tools/tests/test_auto_discard.lua. Fakes never send game traffic.
local asset = 'hook-probe/src/main/assets/liqi_config/ui/MajsoulMaxAutoDiscard.lua'
local count = 0
local function fixture()
    __majmax_auto = nil
    local state = { sent = {}, acks = {}, current = true }
    local function view(name)
        return { valid=true, is_open=false, pai={ToString=function() return name end}, transform={} }
    end
    local normal, red, drawn = view('5m'), view('0m'), view('9p')
    local role = { _can_discard=true, _handpai={normal,red,drawn}, _last_tile=drawn }
    state.role = role
    state.command = {id='one',step=3,seat=0,hand={'5m','5mr','9p'},gameTile='0m',tsumogiri=false,
        autoContext={wind='E',kyoku=1,honba=0}}
    cjson = {encode=function(value) state.acks[#state.acks+1]=value; return 'ack' end,
        decode=function() return state.command end}
    package.loaded.cjson = cjson
    __majmax_auto_command = function() return 'command' end
    __majmax_auto_current = function() return state.current and 'yes' or 'no' end
    __majmax_auto_ack = function() end
    GameUtility = {EMJ_Mode={play=1},E_PlayerOperation={dapai=1,zimo=8,rong=9}}
    MJNetMgr = {SendRequest=function(self,service,method,request,callback,...)
        state.sent[#state.sent+1]=request
        if callback then callback(nil,{error={code=0}}) end
        return 'original'
    end,Inst={IsOK=function() return true end}}
    setmetatable(MJNetMgr.Inst,{__index=MJNetMgr})
    DesktopMgr = { IsActive=function() return true end, Inst={mainrole=role,gameing=true,mode=1,
        seat=1,index_chang=1,index_ju=1,index_ben=0,current_step=3,oplist={{type=1}},game_config={mode={mode=1}}}}
    function role:_setChoosePai(value) self.selected=value end
    function role:_DoDiscardTile()
        MJNetMgr.Inst:SendRequest('FastTest','inputOperation',
            {type=1,tile=self.selected.pai:ToString(),moqie=self.selected==self._last_tile},function() end)
        self._can_discard=false
    end
    dofile(asset)
    return state
end
local function test(name,run)
    run(fixture()); count=count+1; print('PASS '..name)
end
test('red five is distinct and duplicate command is not replayed',function(s)
    __majmax_auto_tick(); __majmax_auto_tick()
    assert(#s.sent==1 and s.sent[1].tile=='0m' and not s.sent[1].moqie)
    assert(s.acks[1].state=='submitted' and s.acks[2].state=='accepted')
end)
test('normal five stays normal',function(s)
    s.command.gameTile='5m'; __majmax_auto_tick(); assert(s.sent[1].tile=='5m')
end)
test('tsumogiri uses the drawn tile',function(s)
    s.command.gameTile='9p'; s.command.tsumogiri=true
    __majmax_auto_tick(); assert(s.sent[1].moqie)
end)
test('incorrect tsumogiri never substitutes another copy',function(s)
    s.command.tsumogiri=true; __majmax_auto_tick(); assert(#s.sent==0 and s.acks[1].state=='illegal_tile')
end)
test('revoked or newer capture never executes',function(s)
    s.current=false; __majmax_auto_tick(); assert(#s.sent==0 and s.acks[1].state=='stale')
end)
test('hand mismatch fails closed',function(s)
    s.command.hand={'5m','5m','9p'}; __majmax_auto_tick(); assert(#s.sent==0 and s.acks[1].state=='hand_mismatch')
end)
test('wrong round fails closed',function(s)
    s.command.autoContext.honba=1; __majmax_auto_tick(); assert(#s.sent==0 and s.acks[1].state=='round_mismatch')
end)
test('draw animation waits for matching game step and legal window',function(s)
    DesktopMgr.Inst.current_step=2; __majmax_auto_tick(); assert(#s.sent==0)
    DesktopMgr.Inst.current_step=3; s.role._can_discard=false; __majmax_auto_tick(); assert(#s.sent==0)
    s.role._can_discard=true; __majmax_auto_tick(); assert(#s.sent==1)
end)
test('invalid selected tile cannot discard',function(s)
    s.role._handpai[2].valid=false; __majmax_auto_tick(); assert(#s.sent==0)
end)
test('win is never silently passed',function(s)
    DesktopMgr.Inst.oplist={{type=1},{type=8}}; __majmax_auto_tick(); assert(#s.sent==0 and s.acks[1].state=='win_available')
end)
test('riichi selection and manual dragging stop automatic input',function(s)
    s.role._during_liqi=true; __majmax_auto_tick(); assert(#s.sent==0 and s.acks[1].state=='special_selection')
end)
test('manual network input is reported and preserves callback',function(s)
    __majmax_auto_tick()
    local called=false
    local result=MJNetMgr.Inst:SendRequest('FastTest','inputChiPengGang',{},function() called=true end)
    assert(result=='original' and called and s.acks[#s.acks].state=='manual')
end)
test('unsupported modes never execute',function(s)
    DesktopMgr.Inst.game_config.mode.mode=5; __majmax_auto_tick(); assert(#s.sent==0)
end)
print(string.format('%d automatic discard checks passed',count))
