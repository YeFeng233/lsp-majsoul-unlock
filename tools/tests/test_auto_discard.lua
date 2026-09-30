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
        action={kind='discard',type=1,tile='0m',moqie=false,method='inputOperation'},
        autoContext={wind='E',kyoku=1,honba=0}}
    cjson = {encode=function(value) state.acks[#state.acks+1]=value; return 'ack' end,
        decode=function() return state.command end}
    package.loaded.cjson = cjson
    __majmax_auto_command = function() return 'command' end
    __majmax_auto_current = function() return state.current and 'yes' or 'no' end
    __majmax_auto_ack = function() end
    GameUtility = {EMJ_Mode={play=1},E_PlayerOperation={dapai=1,eat=2,peng=3,an_gang=4,ming_gang=5,add_gang=6,liqi=7,zimo=8,rong=9,jiuzhongjiupai=10,babei=11}}
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
    local function send(method,op,index,moqie,cancel)
        return MJNetMgr.Inst:SendRequest('FastTest',method,{type=op,index=index,moqie=moqie,cancel_operation=cancel},function() end)
    end
    local function ui()
        local u={transform={gameObject={activeInHierarchy=true}},container_btns={transform={gameObject={activeInHierarchy=true}}}}
        local btn=u.container_btns
        for _,name in ipairs({'lizhi','zimo','hu','gang','babei','jiuzhongjiupai','cancel'}) do
            btn['btn_'..name]={transform={gameObject={activeInHierarchy=true}}}
        end
        return u
    end
    UI_LiqiZimo={Inst=ui()}; UI_ChiPengHu={Inst=ui()}
    local own,claim=UI_LiqiZimo.Inst,UI_ChiPengHu.Inst
    own.com_add_gang={}; own.com_an_gang={}; claim.data={}
    function own.container_btns:Btn_Lizhi() role._during_liqi=true end
    function own.container_btns:Btn_Zimo() send('inputOperation',8,0) end
    function own.container_btns:Btn_Babei() send('inputOperation',11,nil,role._last_tile.pai:ToString()=='4z') end
    function own.container_btns:Btn_JiuZhongJiuPai() send('inputOperation',10,0) end
    function own:OnClickDetail(index)
        send('inputOperation', index < #self.com_add_gang and 6 or 4, index < #self.com_add_gang and index or index-#self.com_add_gang)
    end
    function claim.container_btns:Btn_Gang() send('inputChiPengGang',5,0) end
    function claim.container_btns:Btn_Hu() send('inputChiPengGang',9,0) end
    function claim.container_btns:Btn_Cancel() send('inputChiPengGang',nil,nil,nil,true) end
    function claim:OnClickDetail(index) send('inputChiPengGang',self.choosed_op,index) end
    DesktopMgr.Inst.lastqipai={ToString=function() return '3m' end}; DesktopMgr.Inst.lastqipai_seat=4
    local originalDiscard=role._DoDiscardTile
    function role:_DoDiscardTile()
        if self._during_liqi then
            send('inputOperation',7,nil,self.selected==self._last_tile); self._during_liqi=false
        else originalDiscard(self) end
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
    s.command.gameTile='5m'; s.command.action.tile='5m'; __majmax_auto_tick(); assert(s.sent[1].tile=='5m')
end)
test('tsumogiri uses the drawn tile',function(s)
    s.command.gameTile='9p'; s.command.tsumogiri=true; s.command.action.tile='9p'; s.command.action.moqie=true
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
local function operation(s,kind,op,combo)
    s.command.action={kind=kind,type=op,index=0,target=3,tile='3m',combination=combo,method='inputChiPengGang'}
    DesktopMgr.Inst.oplist={{type=op,combination={combo}}}; s.role._can_discard=false
end
test('riichi sends declaration rather than ordinary discard',function(s)
    s.command.action={kind='riichi',type=7,tile='0m',moqie=false,method='inputOperation'}
    DesktopMgr.Inst.oplist={{type=1},{type=7,combination={'0m'}}}
    DesktopMgr.Inst.liqi_select={s.role._handpai[2].pai}
    __majmax_auto_tick(); assert(#s.sent==1 and s.sent[1].type==7)
end)
test('chi selects exact combination index including red five',function(s)
    operation(s,'chi',2,'0m|4m'); s.command.action.index=1
    DesktopMgr.Inst.oplist[1].combination={'4m|5m','0m|4m'}
    UI_ChiPengHu.Inst.data.chi=DesktopMgr.Inst.oplist[1].combination
    __majmax_auto_tick(); assert(s.sent[1].type==2 and s.sent[1].index==1)
end)
test('pon uses the selected server combination',function(s)
    operation(s,'pon',3,'5m|0m'); UI_ChiPengHu.Inst.data.peng={'5m|0m'}
    __majmax_auto_tick(); assert(s.sent[1].type==3)
end)
test('open kan uses claim window',function(s)
    operation(s,'kan',5,'3m|3m|3m'); __majmax_auto_tick(); assert(s.sent[1].type==5)
end)
test('stale target seat cannot claim',function(s)
    operation(s,'pon',3,'5m|0m'); DesktopMgr.Inst.lastqipai_seat=2
    __majmax_auto_tick(); assert(#s.sent==0 and s.acks[1].state=='target_mismatch')
end)
test('changed combination never substitutes red for normal five',function(s)
    operation(s,'pon',3,'5m|0m'); DesktopMgr.Inst.oplist[1].combination={'5m|5m'}
    __majmax_auto_tick(); assert(#s.sent==0 and s.acks[1].state=='illegal_combination')
end)
test('ankan offsets past kakan entries',function(s)
    operation(s,'ankan',4,'5m|5m|5m|0m')
    UI_LiqiZimo.Inst.com_add_gang={'1p|1p|1p|1p'}; UI_LiqiZimo.Inst.com_an_gang={'5m|5m|5m|0m'}
    __majmax_auto_tick(); assert(s.sent[1].type==4 and s.sent[1].index==0)
end)
test('kakan selects its own zero based index',function(s)
    operation(s,'kakan',6,'5m|5m|5m|0m'); UI_LiqiZimo.Inst.com_add_gang={'5m|5m|5m|0m'}
    __majmax_auto_tick(); assert(s.sent[1].type==6 and s.sent[1].index==0)
end)
test('tsumo uses own turn win button',function(s)
    operation(s,'hora',8); s.command.action.target=0
    __majmax_auto_tick(); assert(s.sent[1].type==8)
end)
test('ron uses claim win button',function(s)
    operation(s,'hora',9); __majmax_auto_tick(); assert(s.sent[1].type==9)
end)
test('pass explicitly cancels claim',function(s)
    operation(s,'pass',0); DesktopMgr.Inst.oplist={{type=2}}
    __majmax_auto_tick(); assert(s.sent[1].cancel_operation)
end)
test('north extraction preserves drawn north flag',function(s)
    operation(s,'kita',11); s.command.action.moqie=false
    s.command.hand[1]='N'; s.role._handpai[1].pai.ToString=function() return '4z' end
    __majmax_auto_tick(); assert(s.sent[1].type==11 and not s.sent[1].moqie)
end)
test('nine terminals uses abort button',function(s)
    operation(s,'abort',10); __majmax_auto_tick(); assert(s.sent[1].type==10)
end)
test('hidden action window waits instead of firing',function(s)
    operation(s,'hora',9); UI_ChiPengHu.Inst.transform.gameObject.activeInHierarchy=false
    __majmax_auto_tick(); assert(#s.sent==0)
end)
print(string.format('%d automatic operation checks passed',count))
