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
            if service ~= 'FastTest' or (method ~= 'inputOperation' and method ~= 'inputChiPengGang') then
                return original(self, service, method, request, callback, ...)
            end
            local id = A.executing
            ack(id, id and 'submitted' or 'manual')
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
local function execute(command)
    local id = command.id
    if type(id) ~= 'string' or #id > 100 or A.seen[id] then return end
    if __majmax_auto_current() ~= 'yes' then mark(id); ack(id, 'stale'); return end
    local d = DesktopMgr and DesktopMgr.Inst
    local r = d and d.mainrole
    local enums = GameUtility and GameUtility.EMJ_Mode
    local ops = GameUtility and GameUtility.E_PlayerOperation
    if not d or not r or not enums or not ops or not d.gameing or not DesktopMgr.IsActive()
            or d.mode ~= enums.play or d.duringReconnect or d.time_stopped or not MJNetMgr.Inst:IsOK() then return end
    -- The network snapshot arrives before the game's draw animation finishes.
    if not r._can_discard or d.current_step ~= command.step then return end
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
    local discard = false
    for _, operation in ipairs(d.oplist or {}) do
        if operation.type == ops.dapai then discard = true end
        -- Do not automatically pass a winning or special selection window.
        if operation.type == ops.zimo or operation.type == ops.rong then mark(id); ack(id, 'win_available'); return end
    end
    if not discard or type(r._DoDiscardTile) ~= 'function' or type(r._setChoosePai) ~= 'function' then return end
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
    if not selected then mark(id); ack(id, 'illegal_tile'); return end
    -- Recheck after all game-side checks and immediately before committing.
    if __majmax_auto_current() ~= 'yes' then mark(id); ack(id, 'stale'); return end
    mark(id)
    A.executing = id
    local ok = pcall(function()
        r:_setChoosePai(selected, false)
        r:_DoDiscardTile()
        if type(r._resetMouseState) == 'function' then r:_resetMouseState() end
    end)
    A.executing = nil
    if not ok then ack(id, 'game_exception') end
end
function __majmax_auto_tick()
    local ok = pcall(function()
        if not install() or type(__majmax_auto_command) ~= 'function' then return end
        local text = __majmax_auto_command()
        if text == '' then return end
        local command = A.json.decode(text)
        if type(command) == 'table' then execute(command) end
    end)
    if not ok then ack('', 'adapter_exception') end
end
