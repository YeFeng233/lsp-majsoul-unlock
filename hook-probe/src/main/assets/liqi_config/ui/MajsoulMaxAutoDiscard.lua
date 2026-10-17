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
-- Keep claim diagnostics bounded to fixed reason codes: no tiles, account data or RPC payloads.
local function claimWaiting(id, reason)
    waiting(id, 'claim_' .. reason)
end
local function claimProbe(id, stage)
    if A.probeId ~= id then A.probeId = id; A.probeStages = {} end
    if A.probeStages[stage] then return end
    A.probeStages[stage] = true
    ack(id, 'probe', 'claim_' .. stage)
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
            if id and A.claimExecuting == id then ack(id, 'probe', 'claim_network_intercepted') end
            local wrapped = function(error, response, ...)
                if id then
                    if A.claimExecuting == id then ack(id, 'probe', 'claim_network_callback') end
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
    local action = command.action
    local claimKind = type(action) == 'table' and (action.kind == 'chi' or action.kind == 'pon')
    -- A newer heartbeat may be captured between provider polling and this tick.
    -- Wait for renewed proof; the controller revokes changed decisions/timeouts.
    if __majmax_auto_current() ~= 'yes' then
        if claimKind then claimWaiting(id, 'capture_proof') else waiting(id, 'capture_proof') end
        return
    end
    local d = DesktopMgr and DesktopMgr.Inst
    local r = d and d.mainrole
    local enums = GameUtility and GameUtility.EMJ_Mode
    local ops = GameUtility and GameUtility.E_PlayerOperation
    if not d or not r or not enums or not ops or not d.gameing or not DesktopMgr.IsActive()
            or d.mode ~= enums.play or d.duringReconnect or d.time_stopped
            or not MJNetMgr or not MJNetMgr.Inst or not MJNetMgr.Inst:IsOK() then
        waiting(id, claimKind and 'claim_game_window' or 'game_window'); return
    end
    -- The network snapshot arrives before the game's draw animation finishes.
    if d.current_step ~= command.step then
        if claimKind then claimWaiting(id, 'game_step')
        else waiting(id, 'game_step '..tostring(d.current_step)..' expected '..tostring(command.step)) end
        return
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
        mark(id)
        if claimKind then ack(id, 'round_mismatch')
        else ack(id, 'round_mismatch', string.format('seat %s/%s wind %s/%s ju %s/%s ben %s/%s',
            tostring(d.seat), tostring(command.seat+1), tostring(d.index_chang), tostring(winds[context.wind]),
            tostring(d.index_ju), tostring(context.kyoku), tostring(d.index_ben), tostring(context.honba))) end
        return
    end
    if type(action) ~= 'table' then mark(id); ack(id, 'unknown_action'); return end
    -- NewGame sorts the complete opening hand; only its first dealer discard
    -- delegates moqie to the original client method. Never relax tile/hand proof.
    local opening = context.openingDealerDiscard == true and action.clientOpeningDraw == true
        and (action.kind == 'discard' or action.kind == 'riichi')
        and context.dealer == command.seat and context.phase == 'wait_act'
        and context.riichi ~= true and type(context.melds) == 'table' and #context.melds == 0
        and type(command.hand) == 'table' and #command.hand == 14
    local kind, operation = action.kind, nil
    for _, op in ipairs(d.oplist or {}) do
        if op.type == action.type then operation = op end
    end
    if kind ~= 'pass' and not operation then
        if kind == 'chi' or kind == 'pon' then claimWaiting(id, 'operation_missing') end
        return
    end
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
            if command.tsumogiri == (view == r._last_tile) or opening then selected = view end
        end
    end
    table.sort(actual); table.sort(expected)
    if #expected == 0 or table.concat(actual, ',') ~= table.concat(expected, ',') then
        mark(id); ack(id, 'hand_mismatch'); return
    end
    if claimKind then claimProbe(id, 'hand_verified') end
    local own, claim
    if claimKind then
        -- Unity property access may throw before the ordinary readiness check can report.
        -- Fixed stage codes only: do not transmit exception strings or game objects.
        local ok
        ok, claim = pcall(function() return UI_ChiPengHu and UI_ChiPengHu.Inst end)
        if not ok then claimWaiting(id, 'ui_instance_exception'); return end
    else
        own = UI_LiqiZimo and UI_LiqiZimo.Inst
        claim = UI_ChiPengHu and UI_ChiPengHu.Inst
    end
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
        local uiReady, isReady = pcall(ready, claim)
        if not uiReady then
            if claimKind then claimWaiting(id, 'ui_readiness_exception') end
            return
        end
        if not isReady then
            if claimKind then claimWaiting(id, 'ui_not_ready') end
            return
        end
        if claimKind then claimProbe(id, 'ui_ready') end
        local targetOk, tileMatches, seatValue = pcall(function()
            if not d.lastqipai or type(d.lastqipai.ToString) ~= 'function' then return nil, nil end
            return d.lastqipai:ToString() == action.tile, d.lastqipai_seat
        end)
        -- Prefer the captured server discard to unstable presentation fields.
        -- Legacy commands without protocol context must still pass the UI guard.
        local source = context.lastDiscard
        if type(source) == 'table' then
            if context.phase ~= 'wait_response' or tile(source.tile) ~= action.tile
                    or source.seat ~= action.target then
                mark(id); ack(id, 'target_mismatch'); return
            end
        elseif not targetOk or not tileMatches or seatValue ~= action.target + 1 then
            mark(id); ack(id, 'target_mismatch'); return
        end
        if claimKind then
            if not targetOk then claimProbe(id, 'target_read_ignored')
            elseif tileMatches == nil then claimProbe(id, 'target_unavailable_ignored')
            elseif not tileMatches then claimProbe(id, 'target_tile_ignored')
            elseif seatValue ~= action.target + 1 then claimProbe(id, 'target_seat_ignored')
            else claimProbe(id, 'target_verified') end
        end
        local index = action.index
        local combinationOk, combinationMatches = pcall(function()
            return type(index) == 'number' and index >= 0 and index % 1 == 0
                and operation.combination and operation.combination[index+1] == action.combination
        end)
        if not combinationOk then
            if claimKind then claimWaiting(id, 'operation_combination_exception') end
            return
        end
        if not combinationMatches then mark(id); ack(id, 'illegal_combination'); return end
        if claimKind then claimProbe(id, 'operation_combination_verified') end
        if kind == 'kan' then
            if index ~= 0 or not button(claim, 'btn_gang', 'Btn_Gang') then return end
            run = function() claim.container_btns:Btn_Gang() end
        else
            local dataOk, values = pcall(function()
                local data = claim.data
                return data and data[kind == 'chi' and 'chi' or 'peng']
            end)
            if not dataOk then claimWaiting(id, 'ui_data_exception'); return end
            if not values then claimWaiting(id, 'ui_data_missing'); return end
            claimProbe(id, 'ui_data_present')
            local valueOk, valueMatches = pcall(function() return values[index+1] == action.combination end)
            if not valueOk then claimWaiting(id, 'ui_combination_exception'); return end
            if not valueMatches then claimWaiting(id, 'ui_combination_mismatch'); return end
            claimProbe(id, 'ui_combination_verified')
            local detailOk, detail = pcall(function() return claim.OnClickDetail end)
            if not detailOk then claimWaiting(id, 'detail_read_exception'); return end
            if type(detail) ~= 'function' then claimWaiting(id, 'detail_unavailable'); return end
            claimProbe(id, 'detail_available')
            run = function()
                claimProbe(id, 'assign_operation_begin')
                claim.choosed_op = action.type
                claimProbe(id, 'assign_operation_done')
                claimProbe(id, 'detail_call_begin')
                detail(claim, index, 1)
                claimProbe(id, 'detail_call_returned')
            end
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
    if claimKind then claimProbe(id, 'commit_ready') end
    -- Recheck after all game-side checks and immediately before committing.
    if __majmax_auto_current() ~= 'yes' then
        if claimKind then claimWaiting(id, 'capture_proof') else waiting(id, 'capture_proof') end
        return
    end
    if claimKind then claimProbe(id, 'capture_verified') end
    mark(id)
    A.executing = id
    if claimKind then A.claimExecuting = id; claimProbe(id, 'run_begin') end
    local ok = pcall(run)
    A.executing = nil
    A.claimExecuting = nil
    if not ok then ack(id, claimKind and 'claim_run_exception' or 'game_exception')
    elseif claimKind then claimProbe(id, 'invoked') end
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
