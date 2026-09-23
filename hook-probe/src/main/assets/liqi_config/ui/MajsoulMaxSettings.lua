-- MajsoulMax in-game settings bootstrap.
--
-- The native side executes this file on Unity's main thread after LuaClient
-- has finished loading. The page renderer deliberately consumes an adapter:
-- the adapter is installed only after the runtime probe identifies the real
-- settings controller and its native templates. That keeps a game update or
-- a missing optional UI bundle from changing the original settings window.
local M = rawget(_G, "__majmax_settings_page") or {}

M.version = "0.7.2"
M.labels = {
    traditional = {
        title = "MOD設置",
        enabled = "開啟 MOD",
        hintSwitch = "強制顯示提示",
        emojiSwitch = "解鎖表情",
        showServer = "顯示伺服器資訊",
        antiNicknameCensorship = "繞過暱稱審查",
        randomCharSwitch = "隨機主角色",
        nickname = "自訂暱稱",
        nicknameHint = "輸入自訂暱稱後確認",
        editNickname = "編輯",
        emptyNickname = "未設定",
        restoreNickname = "恢復原暱稱",
        restart = "重啟遊戲後生效",
    },
    simplified = {
        title = "MOD设置",
        enabled = "开启 MOD",
        hintSwitch = "强制显示提示",
        emojiSwitch = "解锁表情",
        showServer = "显示服务器信息",
        antiNicknameCensorship = "绕过昵称审查",
        randomCharSwitch = "随机主角色",
        nickname = "自定义昵称",
        nicknameHint = "输入自定义昵称后确认",
        editNickname = "编辑",
        emptyNickname = "未设置",
        restoreNickname = "恢复原昵称",
        restart = "重启游戏后生效",
    },
}

M.fields = {
    "enabled",
    "hintSwitch",
    "emojiSwitch",
    "showServer",
    "antiNicknameCensorship",
    "randomCharSwitch",
    "nickname",
}

local function log(message)
    if type(__majmax_ui_log) == "function" then
        __majmax_ui_log(tostring(message))
        return
    end
    if type(print) == "function" then
        print("[MajsoulMax] " .. tostring(message))
    end
end

local function decode(value)
    local json = rawget(_G, "cjson")
    if not json and type(require) == "function" then
        local ok, loaded = pcall(require, "cjson.safe")
        if ok then json = loaded end
    end
    if not json and type(require) == "function" then
        local ok, loaded = pcall(require, "cjson")
        if ok then json = loaded end
    end
    if not json or type(json.decode) ~= "function" then
        return nil, "cjson is unavailable"
    end
    local ok, result, error_message = pcall(json.decode, value)
    if not ok then return nil, result end
    if not result then return nil, error_message or "invalid JSON" end
    return result
end

local function encode(value)
    local json = rawget(_G, "cjson")
    if not json and type(require) == "function" then
        local ok, loaded = pcall(require, "cjson.safe")
        if ok then json = loaded end
    end
    if not json or type(json.encode) ~= "function" then
        return nil, "cjson is unavailable"
    end
    local ok, result = pcall(json.encode, value)
    if not ok then return nil, result end
    return result
end

function M.snapshot()
    if type(__majmax_get_settings) ~= "function" then
        return nil, "native settings bridge unavailable"
    end
    return decode(__majmax_get_settings())
end

function M.update(patch)
    if type(__majmax_update_settings) ~= "function" then
        return nil, "native settings bridge unavailable"
    end
    local encoded = patch
    if type(patch) == "table" then
        local error_message
        encoded, error_message = encode(patch)
        if not encoded then return nil, error_message end
    end
    if type(encoded) ~= "string" then
        return nil, "settings patch must be a table or JSON string"
    end
    local result, error_message = decode(__majmax_update_settings(encoded))
    if result and result.error then return nil, result.error end
    return result, error_message
end

-- Current Android clients send global emo_id values and wait for a server
-- broadcast before rendering. A locally substituted character can therefore
-- lose even its default emojis: the server rejects the ID with code 2208.
-- Keep the real request/response intact and render only that rejection locally.
function M.installEmojiHook()
    local network = rawget(_G, "MJNetMgr")
    if M.emojiHookInstalled or not network or type(network.SendRequest) ~= "function" then
        return M.emojiHookInstalled or false
    end
    local original = network.SendRequest
    network.SendRequest = function(self, service, method, request, callback, ...)
        if service ~= "FastTest" or method ~= "broadcastInGame" then
            return original(self, service, method, request, callback, ...)
        end
        local desktop = rawget(_G, "DesktopMgr") and DesktopMgr.Inst
        local view = rawget(_G, "UI_DesktopInfo") and UI_DesktopInfo.Inst
        local payload = request and type(request.content) == "string" and decode(request.content)
        local emoji_id = type(payload) == "table" and payload.emo_id
        if type(emoji_id) ~= "number" or emoji_id <= 0 or emoji_id > 4294967295
                or emoji_id ~= math.floor(emoji_id) or not desktop or not view then
            return original(self, service, method, request, callback, ...)
        end
        local seat = desktop.seat
        local game_uuid = self.game_uuid
        local player = desktop.player_datas and desktop.player_datas[seat]
        local character_id = player and player.character and player.character.charid
        local handled = false
        local function local_fallback()
            -- A late callback must never display in another game or after the
            -- feature has been disabled. Settings are read at callback time.
            if DesktopMgr.Inst ~= desktop or UI_DesktopInfo.Inst ~= view
                    or self.game_uuid ~= game_uuid or desktop.seat ~= seat
                    or desktop.game_end_result or self.is_ob then return end
            local current_player = desktop.player_datas and desktop.player_datas[seat]
            if not current_player or not current_player.character
                    or current_player.character.charid ~= character_id then return end
            local snapshot = M.snapshot()
            if not snapshot or not snapshot.activeEnabled or not snapshot.settings
                    or not snapshot.settings.enabled or not snapshot.settings.emojiSwitch then return end
            local emoji = rawget(_G, "EmojiMgr")
            if not emoji or type(emoji.IsEmojiAllowed) ~= "function" or not emoji.IsEmojiAllowed()
                    or type(emoji.GetCharEmojiIds) ~= "function"
                    or type(emoji.GetSingleEmojiData) ~= "function"
                    or type(view._onShowEmo) ~= "function"
                    or type(desktop.seat2LocalPosition) ~= "function"
                    or desktop:seat2LocalPosition(seat) ~= 1 then return end
            local belongs_to_character = false
            for _, id in ipairs(emoji.GetCharEmojiIds(character_id) or {}) do
                if id == emoji_id then belongs_to_character = true; break end
            end
            if not belongs_to_character or not emoji.GetSingleEmojiData(emoji_id, character_id) then return end
            view:_onShowEmo(1, emoji_id, nil)
            log("emoji local fallback id=" .. tostring(emoji_id) .. " serverCode=2208")
        end
        local function on_response(error, response, ...)
            local code = response and response.error and response.error.code
            if not handled then
                handled = true
                -- No fallback for transport failures, rate limits, or successful
                -- sends (their normal broadcast must not be shown twice).
                if not error and code == 2208 then
                    local ok = pcall(local_fallback)
                    if not ok then log("emoji local fallback unavailable") end
                end
                if error or (code and code ~= 0) then
                    log("emoji response id=" .. tostring(emoji_id) .. " code=" .. tostring(code)
                        .. " transportError=" .. tostring(error ~= nil and error ~= false))
                end
            end
            if callback then return callback(error, response, ...) end
        end
        return original(self, service, method, request, on_response, ...)
    end
    M.emojiHookInstalled = true
    log("emoji compatibility hook installed")
    return true
end

local function lower(value)
    return string.lower(tostring(value or ""))
end

local function has_setting_word(value)
    local text = lower(value)
    return string.find(text, "setting", 1, true)
        or string.find(text, "option", 1, true)
        or string.find(text, "preference", 1, true)
        or string.find(text, "設置", 1, true)
        or string.find(text, "设置", 1, true)
end

local function safe_index(object, key)
    if not object then return nil end
    local ok, value = pcall(function() return object[key] end)
    if ok then return value end
    return nil
end

local function scan_loaded_modules(result)
    local package_table = rawget(_G, "package")
    local loaded = package_table and package_table.loaded
    if type(loaded) ~= "table" then return end
    for name, module in pairs(loaded) do
        if has_setting_word(name) then
            local keys = {}
            if type(module) == "table" then
                for key in pairs(module) do
                    keys[#keys + 1] = tostring(key)
                    if #keys >= 32 then break end
                end
                table.sort(keys)
            end
            local suffix = #keys > 0 and ("{" .. table.concat(keys, ",") .. "}") or ""
            result[#result + 1] = "lua:" .. tostring(name) .. suffix
        end
        if type(module) == "table" then
            local class_name = safe_index(module, "classFullName")
            if class_name and has_setting_word(class_name) then
                result[#result + 1] = "module:" .. tostring(name) .. "." .. tostring(class_name)
            end
        end
    end
end

local function describe_module(name)
    local package_table = rawget(_G, "package")
    local loaded = package_table and package_table.loaded
    local module = loaded and loaded[name]
    if type(module) ~= "table" then
        return "missing:" .. tostring(name) .. ":" .. type(module)
    end
    local ok, keys = pcall(function()
        local values = {}
        for key in pairs(module) do
            values[#values + 1] = tostring(key)
            if #values >= 64 then break end
        end
        table.sort(values)
        return values
    end)
    if not ok then return "error:" .. tostring(name) .. ":" .. tostring(keys) end
    return tostring(name) .. "{" .. table.concat(keys, ",") .. "}"
end

local function scan_ui_roots(result)
    local unity = rawget(_G, "UnityEngine")
    local capsui = rawget(_G, "capsui")
    if not unity or not capsui then return end
    local object_type = safe_index(unity, "Object")
    local root_type = safe_index(capsui, "CUIRoot")
    if not object_type or not root_type then return end
    local ok, roots = pcall(object_type.FindObjectsOfType, root_type)
    if not ok or not roots then return end
    for index = 0, roots.Length - 1 do
        local root = roots[index]
        local class_name = safe_index(root, "classFullName")
        local prefab = safe_index(root, "prefabPath")
        local text = tostring(class_name or prefab or "")
        if has_setting_word(text) then
            result[#result + 1] = "root:" .. text
        end
    end
end

function M.discover()
    local result = {}
    scan_loaded_modules(result)
    scan_ui_roots(result)
    return result
end

local function describe_instance(instance)
    if type(instance) ~= "table" then return tostring(instance) end
    local fields = {}
    local ok, error_message = pcall(function()
        for key, value in pairs(instance) do
            local text = tostring(key) .. "="
            if type(value) == "table" then
                local nested = {}
                for nested_key in pairs(value) do
                    nested[#nested + 1] = tostring(nested_key)
                    if #nested >= 8 then break end
                end
                table.sort(nested)
                text = text .. "{" .. table.concat(nested, ",") .. "}"
            else
                text = text .. tostring(value)
            end
            fields[#fields + 1] = text
            if #fields >= 48 then break end
        end
    end)
    if not ok then return "<inspect failed: " .. tostring(error_message) .. ">" end
    table.sort(fields)
    return table.concat(fields, ";")
end

function M.observeSettingsInstance(instance, phase)
    local identity = tostring(instance)
    local first_observation = not (M.observed and M.observed[identity])
    M.observed = M.observed or {}
    M.observed[identity] = true
    if first_observation then
        log("UI_Settings " .. tostring(phase) .. " instance=" .. identity)
        local description = describe_instance(instance)
        if #description > 1800 then description = string.sub(description, 1, 1800) end
        log("UI_Settings fields: " .. description)
        for _, field in ipairs({ "tabView", "panels", "tabMenu" }) do
            local value = instance and instance[field]
            if type(value) == "table" then
                local nested = describe_instance(value)
                if #nested > 1600 then nested = string.sub(nested, 1, 1600) end
                log("UI_Settings " .. field .. ": " .. nested)
                if field == "tabView" and type(value.tabList) == "table" then
                    for index, item in pairs(value.tabList) do
                        local item_text = describe_instance(item)
                        if #item_text > 900 then item_text = string.sub(item_text, 1, 900) end
                        log("UI_Settings tabList[" .. tostring(index) .. "]: " .. item_text)
                    end
                end
            end
        end
    end
    local ok, mounted, mount_error = pcall(M.mountSettings, instance)
    if not ok then
        log("MOD settings mount crashed: " .. tostring(mounted))
    elseif not mounted and mount_error then
        log("MOD settings mount deferred: " .. tostring(mount_error))
    end
end

function M.installSettingsObserver()
    local package_table = rawget(_G, "package")
    local loaded = package_table and package_table.loaded
    local module = loaded and loaded["UI.Settings.UI_Settings"]
    if type(module) ~= "table" or module.__majmax_observer then return end
    module.__majmax_observer = true
    local function wrap(name)
        local original = module[name]
        if type(original) ~= "function" then return end
        module[name] = function(instance, ...)
            local values = { ... }
            local ok, first, second, third, fourth = pcall(original, instance,
                unpack(values))
            M.observeSettingsInstance(instance, name)
            if not ok then error(first) end
            return first, second, third, fourth
        end
    end
    wrap("OnCreate")
    wrap("OnShow")
    log("UI_Settings observer installed")
end

function M.installGiftCodePopupHook()
    local package_table = rawget(_G, "package")
    local loaded = package_table and package_table.loaded
    local module = loaded and (loaded["UI.UI_GiftCode"]
        or loaded["UI/UI_GiftCode"] or loaded["UI_GiftCode"])
    module = module or rawget(_G, "UI_GiftCode")
    if type(module) ~= "table" then return false end
    if module.__majmax_popup_hook then return true end
    if type(module.OnShow) ~= "function" or type(module.Confirm) ~= "function" then
        return false
    end

    local original_show = module.OnShow
    local original_close = module.OnClose
    local original_confirm = module.Confirm
    module.Confirm = function(instance, ...)
        if M.nicknamePopupActive and instance == M.nicknamePopupInstance then
            return M.commitNicknamePopup()
        end
        return original_confirm(instance, ...)
    end
    module.OnShow = function(instance, ...)
        local request = M.nicknamePopupRequest
        M.nicknamePopupRequest = nil
        M.resetGiftCodePopup(instance)
        local values = { original_show(instance, ...) }
        if request then
            local ok, error_message = pcall(M.configureNicknamePopup, instance, request)
            if not ok then
                log("nickname popup setup failed: " .. tostring(error_message))
                instance:CloseSelf()
            end
        end
        return unpack(values)
    end
    if type(original_close) == "function" then
        module.OnClose = function(instance, ...)
            local values = { original_close(instance, ...) }
            M.resetGiftCodePopup(instance)
            return unpack(values)
        end
    end
    module.__majmax_popup_hook = true
    return true
end

local function languageLabels()
    local lang = rawget(_G, "LangMgr")
    local preferred = lang and lang.prefer_language
    if preferred == "chs" then return M.labels.simplified end
    return M.labels.traditional
end

local function updateSetting(instance, row, field, value)
    local patch = {}
    patch[field] = value
    local result, error_message = M.update(patch)
    if not result then
        log("setting update failed: " .. tostring(error_message))
        if row and row.SwitchSate then row:SwitchSate(not value, true) end
        return false
    end
    if result.restartRequired then log("setting update requires restart") end
    return true
end

local function addSwitch(group, instance, settings, labels, field, label)
    local row
    row = group:AddSwitcherItem({
        label = label,
        onStr = Tools.StrOfLocalization(26030001),
        offStr = Tools.StrOfLocalization(26030002),
        tipPopInst = instance.tipPopInst,
        initStateOn = settings[field] == true,
        onStateChange = function(value)
            updateSetting(instance, row, field, value)
        end,
    })
    return row
end

function M.closeNicknamePopup()
    local popup = M.nicknamePopupInstance
    if not popup then return false end
    -- Use the same close path as the popup's own X button.
    local ok, result = pcall(function() return popup:CloseSelf() end)
    if not ok then log("nickname popup close failed: " .. tostring(result)) end
    return ok
end

local function setAnchoredPosition(transform, x, y)
    transform.anchoredPosition = UnityEngine.Vector2.New(x, y)
end

local function popupText(popup, path)
    return popup.root:Find(path):GetComponent(typeof(UnityEngine.UI.Text))
end

local function setButtonLabel(button, value)
    button.transform:Find("Text"):GetComponent(typeof(UnityEngine.UI.Text)).text = value
end

function M.resetGiftCodePopup(popup)
    if not popup then return end
    if popup == M.nicknamePopupInstance then
        M.nicknamePopupActive = false
        M.nicknamePopupInstance = nil
        M.nicknamePopupRequestActive = nil
        M.nicknamePopupCommitting = false
    end
    local saved = popup.__majmax_original
    if not saved then return end
    popup.btnConfirm.transform.anchoredPosition = saved.confirmPosition
    popup.input.characterLimit = saved.characterLimit
    popup.input.textComponent.supportRichText = saved.supportRichText
    popupText(popup, "title").text = saved.title
    popupText(popup, "tips").text = saved.tips
    if popup.__majmax_restoreButton then
        popup.__majmax_restoreButton.gameObject:SetActive(false)
    end
    popup.__majmax_original = nil
end

local function saveNickname(value)
    if M.nicknamePopupCommitting then return false end
    local request = M.nicknamePopupRequestActive
    if not request then return false end
    M.nicknamePopupCommitting = true
    local ok, saved = pcall(updateSetting, request.instance, nil, "nickname", value)
    M.nicknamePopupCommitting = false
    if not ok then log("nickname save failed: " .. tostring(saved)) end
    if not ok or not saved then return false end
    if request.row then
        request.row.nicknameValue = value
        if request.row.SetOpenTextStr then
            local display = value == "" and languageLabels().emptyNickname or value
            request.row:SetOpenTextStr(display)
        end
    end
    M.closeNicknamePopup()
    return true
end

function M.restoreNicknamePopup()
    return saveNickname("")
end

function M.commitNicknamePopup()
    local popup = M.nicknamePopupInstance
    if not popup or not M.nicknamePopupActive or popup.locking then return false end
    local value = tostring(popup.input.text or "")
    if value == "" then return false end
    return saveNickname(value)
end

function M.configureNicknamePopup(popup, request)
    if not popup or not request then return end
    local labels = languageLabels()
    local input = popup.input
    local button = popup.btnConfirm
    popup.__majmax_original = {
        confirmPosition = button.transform.anchoredPosition,
        characterLimit = input.characterLimit,
        supportRichText = input.textComponent.supportRichText,
        title = popupText(popup, "title").text,
        tips = popupText(popup, "tips").text,
    }
    M.nicknamePopupActive = true
    M.nicknamePopupInstance = popup
    M.nicknamePopupRequestActive = request

    -- Bind once per instance. uGUI InputField has onValueChanged, but does not
    -- expose TMP's onSelect. Retain 4.0.16's gift-code whitespace filtering only
    -- outside nickname mode; nickname text is saved without that transformation.
    if not popup.__majmax_inputHook then
        input.onValueChanged:RemoveAllListeners()
        input.onValueChanged:AddListener(function(value)
            if not M.nicknamePopupActive or M.nicknamePopupInstance ~= popup then
                input.text = tostring(value or ""):gsub("[%s\n]", "")
            end
            popup:RefreshConfirmBtn()
        end)
        popup.__majmax_inputHook = true
    end
    input.characterLimit = 64
    input.textComponent.supportRichText = false
    input.text = request.value or ""
    popupText(popup, "title").text = labels.nickname
    popupText(popup, "tips").text = labels.nicknameHint

    local restore = popup.__majmax_restoreButton
    if not restore then
        local object = UnityEngine.GameObject.Instantiate(button.gameObject, button.transform.parent)
        object.name = "BtnRestoreNickname"
        restore = object:GetComponent(typeof(UnityEngine.UI.Button))
        restore.onClick:RemoveAllListeners()
        restore.onClick:AddListener(function()
            if M.nicknamePopupInstance == popup and not popup.locking then
                M.restoreNicknamePopup()
            end
        end)
        popup.__majmax_restoreButton = restore
    end
    restore.gameObject:SetActive(true)
    restore.interactable = true
    restore:GetComponent(typeof(capsui.ImageSwitcher)):SwitchImage(1)
    setButtonLabel(restore, labels.restoreNickname)
    local origin = popup.__majmax_original.confirmPosition
    local offset = (button.transform.sizeDelta.x + 16) / 2
    setAnchoredPosition(button.transform, origin.x + offset, origin.y)
    setAnchoredPosition(restore.transform, origin.x - offset, origin.y)
    -- The original click listener dispatches through our Confirm hook. Keeping
    -- it also preserves normal gift-code behavior when the popup is reused.
    popup:RefreshConfirmBtn()
end

function M.openNicknamePopup(instance, row, value)
    local gift = rawget(_G, "UI_GiftCode")
    local manager = rawget(_G, "UIMgr")
    if not gift or not manager or not manager.Inst or not M.installGiftCodePopupHook() then
        log("nickname popup unavailable")
        return
    end
    if M.nicknamePopupActive or M.nicknamePopupRequest then return end
    M.nicknamePopupRequest = { instance = instance, row = row, value = value or "" }
    local ok, error_message = pcall(function() manager.Inst:ShowUIBase(gift) end)
    if not ok then
        M.nicknamePopupRequest = nil
        log("nickname popup failed: " .. tostring(error_message))
    end
    -- ShowUIBase is asynchronous. Only OnShow consumes the request, after the
    -- original window has initialized; configuring gift.Inst here is too early.
end

local function addNicknameRows(group, instance, settings, labels)
    local current = settings.nickname or ""
    local row
    row = group:AddOpenSubPageItem({
        label = labels.nickname,
        openDesc = current == "" and labels.emptyNickname or current,
        onOpenClick = function()
            M.openNicknamePopup(instance, row, row.nicknameValue or current)
        end,
    })
    row.nicknameValue = current
    return row
end

function M.mountSettings(instance)
    if M.mountedInstance == instance then return true end
    if type(instance) ~= "table" or not instance.tabView or not instance.panels then
        return false, "settings instance is incomplete"
    end
    local labels = languageLabels()
    local settings, error_message = M.snapshot()
    if not settings or not settings.settings then
        return false, error_message or "settings snapshot is unavailable"
    end
    settings = settings.settings
    local tabView = instance.tabView
    local panelRoot = instance.otherPanelRoot
    if not panelRoot or not panelRoot.parent then return false, "settings panel root is missing" end
    local pageRoot = UnityEngine.GameObject.Instantiate(panelRoot, panelRoot.parent)
    pageRoot.name = "MajsoulMaxSettingsPanel"
    pageRoot.gameObject:SetActive(false)
    local groupRoot = pageRoot:Find("Scroll View/Viewport/Content/OtherGroup")
    local scroll = pageRoot:Find("Scroll View"):GetComponent(typeof(UnityEngine.UI.ScrollRect))
    if not groupRoot or not scroll or not FormGroupBase then
        return false, "native form group template is missing"
    end
    for index = 0, groupRoot.childCount - 1 do
        groupRoot:GetChild(index).gameObject:SetActive(false)
    end
    local group = FormGroupBase:New({ trans = groupRoot })
    local rows = {}
    rows.enabled = addSwitch(group, instance, settings, labels, "enabled", labels.enabled)
    rows.hintSwitch = addSwitch(group, instance, settings, labels, "hintSwitch", labels.hintSwitch)
    rows.emojiSwitch = addSwitch(group, instance, settings, labels, "emojiSwitch", labels.emojiSwitch)
    rows.showServer = addSwitch(group, instance, settings, labels, "showServer", labels.showServer)
    rows.antiNicknameCensorship = addSwitch(group, instance, settings, labels,
        "antiNicknameCensorship", labels.antiNicknameCensorship)
    rows.randomCharSwitch = addSwitch(group, instance, settings, labels,
        "randomCharSwitch", labels.randomCharSwitch)
    rows.nickname = addNicknameRows(group, instance, settings, labels)
    UITool.SetFontLocalizationWithinRoot(groupRoot)

    local item = {}
    item.index = #tabView.tabList + 1
    item.trans = UnityEngine.GameObject.Instantiate(tabView.tabItemTemplate, tabView.tabMenu).transform
    item.trans.gameObject:SetActive(true)
    item.trans.name = "tab_MajsoulMax"
    item.label = item.trans:Find("Label"):GetComponent(typeof(UnityEngine.UI.Text))
    item.label.text = labels.title
    item.activeBg = item.trans:Find("Active"):GetComponent(typeof(UnityEngine.UI.Image))
    item.normalBg = item.trans:Find("Normal"):GetComponent(typeof(UnityEngine.UI.Image))
    item.btn = item.trans:GetComponent(typeof(UnityEngine.UI.Button))
    item.btn.onClick:AddListener(function() tabView:OnTabClick(item.index) end)
    table.insert(tabView.tabList, item)

    local panel = {}
    panel.trans = pageRoot
    panel.tabLabel = labels.title
    panel.scroll = scroll
    function panel:Show()
        self.trans.gameObject:SetActive(true)
        self.scroll.verticalNormalizedPosition = 1
    end
    function panel:Hide()
        self.trans.gameObject:SetActive(false)
    end
    function panel:Refresh() end
    table.insert(instance.panels, panel)
    M.mountedInstance = instance
    M.page = pageRoot
    M.rows = rows
    log("MOD settings tab mounted")
    if type(__majmax_ui_ready) == "function" then __majmax_ui_ready() end
    return true
end

-- Adapter contract used by the eventual native-template page. Each method is
-- called on Unity's main thread and must return the game-owned component it
-- created. No nickname text is interpolated into Lua source.
function M.attach(adapter)
    if type(adapter) ~= "table" then return nil, "settings adapter is unavailable" end
    if type(adapter.createPage) ~= "function"
            or type(adapter.addToggle) ~= "function"
            or type(adapter.addInput) ~= "function" then
        return nil, "settings adapter is incomplete"
    end
    if M.attached then return true end
    local snapshot, error_message = M.snapshot()
    if not snapshot or not snapshot.settings then
        return nil, error_message or "settings snapshot is unavailable"
    end
    local settings = snapshot.settings
    local page = adapter:createPage(M.labels)
    if not page then return nil, "adapter could not create the MOD page" end
    local function save(field, value)
        local patch = {}
        patch[field] = value
        local updated, update_error = M.update(patch)
        if not updated then
            if type(adapter.showError) == "function" then adapter:showError(update_error) end
            return false
        end
        return true
    end
    adapter:addToggle(page, "enabled", settings.enabled, function(value) return save("enabled", value) end)
    adapter:addToggle(page, "hintSwitch", settings.hintSwitch, function(value) return save("hintSwitch", value) end)
    adapter:addToggle(page, "emojiSwitch", settings.emojiSwitch, function(value) return save("emojiSwitch", value) end)
    adapter:addToggle(page, "showServer", settings.showServer, function(value) return save("showServer", value) end)
    adapter:addToggle(page, "antiNicknameCensorship", settings.antiNicknameCensorship,
        function(value) return save("antiNicknameCensorship", value) end)
    adapter:addToggle(page, "randomCharSwitch", settings.randomCharSwitch,
        function(value) return save("randomCharSwitch", value) end)
    adapter:addInput(page, "nickname", settings.nickname or "", function(value) return save("nickname", value) end)
    if type(adapter.addRestoreNickname) == "function" then
        adapter:addRestoreNickname(page, function() return save("nickname", "") end)
    end
    M.attached = true
    if type(__majmax_ui_ready) == "function" then __majmax_ui_ready() end
    return true
end

function M.bootstrap()
    M.installEmojiHook()
    if M.attached then return true end
    if not M.started then
        M.started = true
        log("bootstrap running")
    end
    M.installSettingsObserver()
    M.installGiftCodePopupHook()
    local adapter = rawget(_G, "__majmax_settings_adapter")
    if type(adapter) == "table" then
        local ok, error_message = M.attach(adapter)
        if not ok then log("UI adapter: " .. tostring(error_message)) end
        return ok
    end
    M.scanCount = (M.scanCount or 0) + 1
    if M.scanCount == 1 or M.scanCount % 10 == 0 then
        local discovered, candidates = pcall(M.discover)
        if not discovered then
            log("discover failed: " .. tostring(candidates))
            return false
        end
        log("discover count: " .. tostring(#candidates))
        local described, value = pcall(describe_module, "UI.Settings.UI_Settings")
        candidates[#candidates + 1] = described and value or ("describe failed: " .. tostring(value))
        described, value = pcall(describe_module, "UI.Settings.UI_SettingsOtherPanel")
        candidates[#candidates + 1] = described and value or ("describe failed: " .. tostring(value))
        described, value = pcall(describe_module, "UIModels.UI_Settings.SettingMainModel")
        candidates[#candidates + 1] = described and value or ("describe failed: " .. tostring(value))
        for _, candidate in ipairs(candidates) do
            log("settings UI candidate: " .. candidate)
        end
    end
    return false
end

_G.__majmax_settings_page = M
local ok, error_message = pcall(M.bootstrap)
if not ok then log("bootstrap failed: " .. tostring(error_message)) end
