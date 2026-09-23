-- Run from the repository root: lua5.1 tools/tests/test_emoji_compat.lua
-- The network callback and game UI are fakes; no requests leave this process.
local asset = "hook-probe/src/main/assets/liqi_config/ui/MajsoulMaxSettings.lua"
local count = 0
local function fixture(content)
    local state = { sent = {}, shown = {}, calls = 0, logs = {}, allowed = true }
    state.snapshot = { activeEnabled = true, settings = { enabled = true, emojiSwitch = true } }
    -- JSON parsing is provided by the game's cjson module. Test only its inputs
    -- and decoded outcomes here, including a decoding exception.
    cjson = { decode = function(value)
        if value == "snapshot" then return state.snapshot end
        if value == '{"emo_id":1230000}' then return { emo_id = 1230000 } end
        if value == '{"emo_id":1230010}' then return { emo_id = 1230010 } end
        if value == '{"emo_id":1010000}' then return { emo_id = 1010000 } end
        if value == '{"emo_id":"1230000"}' then return { emo_id = "1230000" } end
        if value == '{"emo_id":1.5}' then return { emo_id = 1.5 } end
        if value == '{"emo":0}' then return { emo = 0 } end
        error("invalid JSON")
    end }
    __majmax_get_settings = function() return "snapshot" end
    __majmax_ui_log = function(message) state.logs[#state.logs + 1] = message end
    __majmax_settings_page = { attached = true }
    MJNetMgr = { game_uuid = "first-game", SendRequest = function(self, service, method, request, callback, extra)
        state.sent[#state.sent + 1] = { request = request, callback = callback, service = service, method = method }
        assert(extra == "request-extra")
        return "original-return"
    end }
    DesktopMgr = { Inst = { seat = 3, player_datas = { [3] = { character = { charid = 20000123 } } },
        seat2LocalPosition = function(self, seat) assert(seat == 3); return 1 end } }
    UI_DesktopInfo = { Inst = { _onShowEmo = function(self, position, id, sub)
        state.shown[#state.shown + 1] = { position = position, id = id, sub = sub }
    end } }
    EmojiMgr = {
        IsEmojiAllowed = function() return state.allowed end,
        GetCharEmojiIds = function(charid) assert(charid == 20000123); return {1230000, 1230010} end,
        GetSingleEmojiData = function(id, charid)
            if state.missing then return nil end
            return { imagePath = "test" }
        end,
    }
    dofile(asset)
    local hooked = MJNetMgr.SendRequest
    __majmax_settings_page.bootstrap()
    assert(MJNetMgr.SendRequest == hooked, "bootstrap must not stack wrappers")
    state.request = { content = content or '{"emo_id":1230000}', except_self = false }
    state.callback = function(error, response, extra)
        state.calls = state.calls + 1
        state.error, state.response = error, response
        assert(extra == "callback-extra")
        return "callback-return"
    end
    function state:send(service, method)
        assert(MJNetMgr:SendRequest(service or "FastTest", method or "broadcastInGame",
            self.request, self.callback, "request-extra") == "original-return")
        assert(#self.sent == 1 and self.sent[1].request == self.request)
    end
    function state:respond(code, transport)
        local response = { error = { code = code } }
        assert(self.sent[1].callback(transport, response, "callback-extra") == "callback-return")
        assert(self.response == response and self.error == transport)
    end
    return state
end
local function test(name, run)
    run()
    count = count + 1
    print("ok " .. count .. " - " .. name)
end

test("default and extra global IDs rejected with 2208 display locally once", function()
    for _, id in ipairs({1230000, 1230010}) do
        local s = fixture('{"emo_id":' .. id .. '}')
        s:send(); s:respond(2208); s:respond(2208)
        assert(#s.shown == 1 and s.shown[1].id == id and s.shown[1].position == 1)
        assert(s.calls == 2 and s.request.except_self == false)
    end
end)
test("successful sends and unrelated rejections keep original behavior", function()
    for _, code in ipairs({0, 1, 2207, 2209}) do
        local s = fixture(); s:send(); s:respond(code)
        assert(#s.shown == 0 and s.calls == 1)
    end
end)
test("transport failures never fabricate delivery", function()
    local s = fixture(); s:send(); s:respond(2208, "timeout"); assert(#s.shown == 0)
end)
test("feature changes while a request is pending are respected", function()
    for _, field in ipairs({"activeEnabled", "enabled", "emojiSwitch"}) do
        local s = fixture(); s:send()
        if field == "activeEnabled" then s.snapshot[field] = false else s.snapshot.settings[field] = false end
        s:respond(2208); assert(#s.shown == 0)
    end
end)
test("callbacks from an old game, UI, seat, or character cannot display", function()
    local changes = {
        function() MJNetMgr.game_uuid = "second-game" end,
        function() DesktopMgr.Inst = {} end,
        function() UI_DesktopInfo.Inst = {} end,
        function() DesktopMgr.Inst.seat = 1 end,
        function() DesktopMgr.Inst.player_datas[3].character.charid = 20000101 end,
    }
    for _, change in ipairs(changes) do
        local s = fixture(); s:send(); change(); s:respond(2208); assert(#s.shown == 0)
    end
end)
test("observer, completed games, and rooms with emojis disabled are excluded", function()
    for _, change in ipairs({
        function(s) MJNetMgr.is_ob = true end,
        function(s) DesktopMgr.Inst.game_end_result = {} end,
        function(s) s.allowed = false end,
    }) do
        local s = fixture(); s:send(); change(s); s:respond(2208); assert(#s.shown == 0)
    end
end)
test("malformed, legacy, and other-character content passes through", function()
    for _, content in ipairs({'bad json', '{"emo":0}', '{"emo_id":"1230000"}',
            '{"emo_id":1.5}', '{"emo_id":1010000}'}) do
        local s = fixture(content); s:send(); s:respond(2208); assert(#s.shown == 0)
    end
end)
test("other RPCs retain their original callback", function()
    local s = fixture(); s:send("Lobby", "loginBeat")
    assert(s.sent[1].callback == s.callback)
end)
test("missing display API or a display exception does not swallow callbacks", function()
    for _, replacement in ipairs({ false, function() error("UI unavailable") end }) do
        local s = fixture(); s:send(); UI_DesktopInfo.Inst._onShowEmo = replacement
        s:respond(2208); assert(s.calls == 1 and #s.shown == 0)
    end
end)
test("unavailable game resources and incomplete successful responses are ignored", function()
    local s = fixture(); s:send(); s.missing = true; s:respond(2208)
    assert(#s.shown == 0 and s.calls == 1)
    s = fixture(); s:send(); s:respond(nil)
    assert(#s.shown == 0 and s.calls == 1)
end)
print("passed " .. count .. " emoji compatibility tests")
