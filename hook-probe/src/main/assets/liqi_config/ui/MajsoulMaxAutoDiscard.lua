-- Commands are data, never evaluated as Lua. This file runs only on Unity's main thread.
local A = rawget(_G, '__majmax_auto') or { seen = {}, order = {} }
_G.__majmax_auto = A
local function ack(id, state, reason)
    if type(__majmax_auto_ack) == 'function' and A.json then
        __majmax_auto_ack(A.json.encode({id = id or '', state = state, reason = reason or ''}))
    end
end
local function mark(id)
    A.seen[id] = true
    A.order[#A.order + 1] = id
    if #A.order > 64 then A.seen[table.remove(A.order, 1)] = nil end
end
local function waiting(id, reason)
    if A.waitId ~= id or A.waitReason ~= reason then
        A.waitId = id; A.waitReason = reason
        ack(id, 'waiting', reason)
    end
end
local function tile(value)
    local honors = { E='1z', S='2z', W='3z', N='4z', P='5z', F='6z', C='7z' }
    if honors[value] then return honors[value] end
    if type(value) ~= 'string' then return nil end
    if value:match('^5[mps]r$') then return '0'..value:sub(2,2) end
    if value:match('^[1-9][mps]$') then return value end
end
local function install()
    if not A.json then local ok, json = pcall(require, 'cjson'); if ok then A.json = json end end
    if not A.json or not MJNetMgr or type(MJNetMgr.SendRequest) ~= 'function' then return false end
    if not A.networkHooked then
        A.networkHooked = true
        local original = MJNetMgr.SendRequest
        function MJNetMgr:SendRequest(service, method, request, callback, ...)
            if service == 'FastTest' and method == 'syncGame' and A.resyncing then
                -- Rebuild the assistant from auth + the complete round, not a partial delta.
                request.round_id = '-1'; request.step = 1000000
                A.resyncing = nil
            end
            if service ~= 'FastTest' or (method ~= 'inputOperation' and method ~= 'inputChiPengGang') then
                return original(self, service, method, request, callback, ...)
            end
            local id = A.executing
            local external = request and (request.auto_operation or (tonumber(request.timeuse) or 0) >= 1000000)
            ack(id, id and 'submitted' or (external and 'external' or 'manual'))
            local wrapped = function(error, response, ...)
                if id then
                    local code = response and response.error and response.error.code
                    ack(id, (not error and response and (not code or code == 0)) and 'accepted' or 'failed')
                end
                if callback then return callback(error, response, ...) end
            end
            return original(self, service, method, request, wrapped, ...)
        end
    end
    return true
end
local function active(object)
    if not object then return false end
    local gameObject = object.gameObject or (object.transform and object.transform.gameObject)
    return gameObject and gameObject.activeInHierarchy
end
local function seconds()
    return UnityEngine and UnityEngine.Time and UnityEngine.Time.realtimeSinceStartup
end
local function continuation(command)
    local t = seconds()
    local d = DesktopMgr and DesktopMgr.Inst
    local net = MJNetMgr and MJNetMgr.Inst
    local mode = d and d.game_config and d.game_config.mode and d.game_config.mode.mode
    if not A.armed or not t or not d or not DesktopMgr.IsActive() or not GameUtility
            or d.mode ~= GameUtility.EMJ_Mode.play or d.duringReconnect or d.time_stopped
            or not net or not net:IsOK() or (mode ~= 1 and mode ~= 2 and mode ~= 11 and mode ~= 12) then
        A.continuation = nil; A.resyncDue = nil; return
    end
    -- Only known game result/next-round controls; never starts a new match.
    local candidates = {
        {UI_Win, '_onConfirm'}, {UI_HuleShow, 'Btn_Confirm'},
        {UI_ScoreChange, 'Btn_Confirm'}, {UI_ConfirmNewRound, 'Btn_Confirm'},
        {UI_HangUpWarn, 'Btn_Confirm', true}
    }
    for _, item in ipairs(candidates) do
        local class, method, root = item[1], item[2], item[3]
        if class and type(class.OnShow) == 'function' and not class._majmaxShowHook then
            class._majmaxShowHook = true
            local original = class.OnShow
            function class:OnShow(...)
                self._majmaxShow = (self._majmaxShow or 0) + 1
                return original(self, ...)
            end
        end
        local ui = class and class.Inst
        local owner = ui and (root and ui.root or ui)
        local btn = owner and owner.btn_confirm
        if ui and owner and active(ui) and active(btn) and btn.interactable ~= false
                and not ui.locking and not owner.locking and not ui.isDoAnimation
                and type(owner[method]) == 'function' then
            local key = tostring(ui._majmaxShow or 0)..':'..tostring(ui.current_index or 0)
                ..':'..tostring(ui.during_show_liujumanguan or false)
            local pending = A.continuation
            if not pending or pending.ui ~= ui or pending.key ~= key then
                pending = {ui=ui, key=key, due=t + math.random(2000,5000)/1000}
                A.continuation = pending
            end
            A.resyncDue = nil
            if not pending.done and t >= pending.due then
                pending.done = true
                local ok = pcall(owner[method], owner)
                ack('', ok and 'continued' or 'continuation_failed')
            end
            return
        end
    end
    A.continuation = nil
    if command.control == 'mode' and command.resync and d.gameing
            and type(net._connectSuccess) == 'function' and t >= (A.nextResync or 0) then
        A.resyncDue = A.resyncDue or (t + math.random(2000,5000)/1000)
        if t >= A.resyncDue then
            A.resyncDue = nil; A.nextResync = t + 30; A.resyncing = true
            local ok = pcall(net._connectSuccess, net)
            if not ok then A.resyncing = nil end
            ack('', ok and 'resync' or 'resync_failed')
        end
    else A.resyncDue = nil end
end
local function execute(command)
    local id = command.id
    if type(id) ~= 'string' or #id > 100 or A.seen[id] then return end
    -- A newer heartbeat may be captured between provider polling and this tick.
    -- Wait for renewed proof; the controller revokes changed decisions/timeouts.
    if __majmax_auto_current() ~= 'yes' then waiting(id, 'capture_proof'); return end
    local d = DesktopMgr and DesktopMgr.Inst
    local r = d and d.mainrole
    local enums = GameUtility and GameUtility.EMJ_Mode
    local ops = GameUtility and GameUtility.E_PlayerOperation
    if not d or not r or not enums or not ops or not d.gameing or not DesktopMgr.IsActive()
            or d.mode ~= enums.play or d.duringReconnect or d.time_stopped or not MJNetMgr.Inst:IsOK() then
        waiting(id, 'game_window'); return
    end
    -- The network snapshot arrives before the game's draw animation finishes.
    if d.current_step ~= command.step then
        waiting(id, 'game_step '..tostring(d.current_step)..' expected '..tostring(command.step)); return
    end
    if r._mouse_downed or r._during_drag then mark(id); ack(id, 'manual'); return end
    if r._during_liqi or r._during_reveal or r._during_reveal_liqi then
        mark(id); ack(id, 'special_selection'); return
    end
    local mode = d.game_config and d.game_config.mode and d.game_config.mode.mode
    if mode ~= 1 and mode ~= 2 and mode ~= 11 and mode ~= 12 then mark(id); ack(id, 'unsupported_mode'); return end
    local context = command.autoContext
    -- DesktopMgr uses 1-based round winds; Akagi exposes mjai wind names.
    local winds = {E=1, S=2, W=3, N=4}
    if type(context) ~= 'table' or d.seat ~= command.seat + 1 or d.index_chang ~= winds[context.wind]
            or d.index_ju ~= context.kyoku or d.index_ben ~= context.honba then
        mark(id); ack(id, 'round_mismatch', string.format('seat %s/%s wind %s/%s ju %s/%s ben %s/%s',
            tostring(d.seat), tostring(command.seat+1), tostring(d.index_chang), tostring(winds[context.wind]),
            tostring(d.index_ju), tostring(context.kyoku), tostring(d.index_ben), tostring(context.honba))); return
    end
    local action = command.action
    if type(action) ~= 'table' then mark(id); ack(id, 'unknown_action'); return end
    local kind, operation = action.kind, nil
    for _, op in ipairs(d.oplist or {}) do
        if op.type == action.type then operation = op end
    end
    if kind ~= 'pass' and not operation then return end
    if kind == 'discard' or kind == 'riichi' then
        if not r._can_discard or type(r._DoDiscardTile) ~= 'function' or type(r._setChoosePai) ~= 'function' then return end
        -- A winning window must be handled by the explicit AI recommendation.
        for _, op in ipairs(d.oplist or {}) do
            if kind == 'discard' and (op.type == ops.zimo or op.type == ops.rong) then
                mark(id); ack(id, 'win_available'); return
            end
        end
    end
    local actual, expected, selected = {}, {}, nil
    for _, value in ipairs(command.hand or {}) do
        local name = tile(value)
        if not name then mark(id); ack(id, 'invalid_hand'); return end
        expected[#expected + 1] = name
    end
    for _, view in ipairs(r._handpai or {}) do
        if not view.pai or type(view.pai.ToString) ~= 'function' then mark(id); ack(id, 'unknown_hand'); return end
        local name = view.pai:ToString()
        actual[#actual + 1] = name
        if name == command.gameTile and view.valid and not view.pai.baida and not view.is_open then
            if command.tsumogiri == (view == r._last_tile) then selected = view end
        end
    end
    table.sort(actual); table.sort(expected)
    if #expected == 0 or table.concat(actual, ',') ~= table.concat(expected, ',') then
        mark(id); ack(id, 'hand_mismatch'); return
    end
    local own = UI_LiqiZimo and UI_LiqiZimo.Inst
    local claim = UI_ChiPengHu and UI_ChiPengHu.Inst
    local function ready(ui)
        return ui and ui.transform and ui.transform.gameObject.activeInHierarchy and not ui.on_do_operation
            and ui.container_btns and ui.container_btns.transform.gameObject.activeInHierarchy
    end
    local function button(ui, name, method)
        local btn = ui and ui.container_btns and ui.container_btns[name]
        return ready(ui) and btn and btn.transform.gameObject.activeInHierarchy
            and type(ui.container_btns[method]) == 'function'
    end
    local run
    if kind == 'discard' or kind == 'riichi' then
        if not selected then mark(id); ack(id, 'illegal_tile'); return end
        if kind == 'riichi' then
            if not button(own, 'btn_lizhi', 'Btn_Lizhi') then return end
            local found = false
            for _, value in ipairs(operation.combination or {}) do if value:gsub('0','5') == action.tile:gsub('0','5') then found = true end end
            if not found then mark(id); ack(id, 'illegal_combination'); return end
            -- DesktopMgr builds liqi_select from the current server operation list.
            local valid = false
            for _, value in ipairs(d.liqi_select or {}) do if value:ToString():gsub('0','5') == action.tile:gsub('0','5') then valid = true end end
            if not valid then return end
        end
        run = function()
            if kind == 'riichi' then
                own.container_btns:Btn_Lizhi(ops.liqi)
                if not r._during_liqi then error('riichi selection did not activate') end
            end
            if not selected.valid then error('riichi tile unavailable') end
            r:_setChoosePai(selected, false); r:_DoDiscardTile()
            if type(r._resetMouseState) == 'function' then r:_resetMouseState() end
        end
    elseif kind == 'chi' or kind == 'pon' or kind == 'kan' then
        if not ready(claim) then return end
        if not d.lastqipai or d.lastqipai:ToString() ~= action.tile or d.lastqipai_seat ~= action.target + 1 then
            mark(id); ack(id, 'target_mismatch'); return
        end
        local index = action.index
        if type(index) ~= 'number' or index < 0 or index % 1 ~= 0
                or (operation.combination or {})[index+1] ~= action.combination then
            mark(id); ack(id, 'illegal_combination'); return
        end
        if kind == 'kan' then
            if index ~= 0 or not button(claim, 'btn_gang', 'Btn_Gang') then return end
            run = function() claim.container_btns:Btn_Gang() end
        else
            local values = claim.data and claim.data[kind == 'chi' and 'chi' or 'peng']
            if not values or values[index+1] ~= action.combination then return end
            if type(claim.OnClickDetail) ~= 'function' then return end
            run = function() claim.choosed_op = action.type; claim:OnClickDetail(index, 1) end
        end
    elseif kind == 'ankan' or kind == 'kakan' then
        if not ready(own) or type(own.OnClickDetail) ~= 'function' then return end
        local values = kind == 'ankan' and own.com_an_gang or own.com_add_gang
        local index = action.index
        if type(index) ~= 'number' or index < 0 or index % 1 ~= 0
                or (operation.combination or {})[index+1] ~= action.combination then
            mark(id); ack(id, 'illegal_combination'); return
        end
        if not values or values[index+1] ~= action.combination then return end
        local offset = kind == 'ankan' and #(own.com_add_gang or {}) or 0
        run = function() own:OnClickDetail(index + offset) end
    elseif kind == 'hora' then
        if action.target == command.seat and action.type == ops.zimo then
            if not button(own, 'btn_zimo', 'Btn_Zimo') then return end
            run = function() own.container_btns:Btn_Zimo() end
        elseif action.target ~= command.seat and action.type == ops.rong then
            if not button(claim, 'btn_hu', 'Btn_Hu') then return end
            run = function() claim.container_btns:Btn_Hu() end
        end
    elseif kind == 'kita' then
        if not button(own, 'btn_babei', 'Btn_Babei') then return end
        local north = false
        for _, value in ipairs(actual) do if value == '4z' then north = true end end
        if not north or action.moqie ~= (r._last_tile ~= nil and r._last_tile.pai:ToString() == '4z') then
            mark(id); ack(id, 'illegal_tile'); return
        end
        run = function() own.container_btns:Btn_Babei() end
    elseif kind == 'abort' then
        if not button(own, 'btn_jiuzhongjiupai', 'Btn_JiuZhongJiuPai') then return end
        run = function() own.container_btns:Btn_JiuZhongJiuPai() end
    elseif kind == 'pass' then
        if action.method ~= 'inputChiPengGang' or not button(claim, 'btn_cancel', 'Btn_Cancel') then return end
        run = function() claim.container_btns:Btn_Cancel() end
    end
    if not run then mark(id); ack(id, 'unknown_action'); return end
    -- Recheck after all game-side checks and immediately before committing.
    if __majmax_auto_current() ~= 'yes' then waiting(id, 'capture_proof'); return end
    mark(id)
    A.executing = id
    local ok = pcall(run)
    A.executing = nil
    if not ok then ack(id, 'game_exception') end
end
function __majmax_auto_tick()
    local ok = pcall(function()
        if not install() or type(__majmax_auto_command) ~= 'function' then return end
        local text = __majmax_auto_command()
        if not text or text == '' then A.armed = false; A.continuation = nil; A.resyncDue = nil; return end
        local command = A.json.decode(text)
        if type(command) == 'table' then
            A.armed = (command.control == 'mode' and command.enabled == true) or command.unattended == true
            continuation(command)
            if command.control ~= 'mode' then execute(command) end
        end
    end)
    if not ok then ack('', 'adapter_exception') end
end
