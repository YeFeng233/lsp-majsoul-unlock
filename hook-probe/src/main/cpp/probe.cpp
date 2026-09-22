#include <android/log.h>
#include <dlfcn.h>
#include <jni.h>
#include <pthread.h>
#include <unistd.h>

#include <atomic>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <fstream>
#include <iterator>
#include <mutex>
#include <chrono>
#include <string>
#include <unordered_map>
#include <utility>
#include <vector>

namespace {
constexpr const char *kTag = "MajsoulHook";
#define INFO(...) __android_log_print(ANDROID_LOG_INFO, kTag, __VA_ARGS__)
#define WARN(...) __android_log_print(ANDROID_LOG_WARN, kTag, __VA_ARGS__)
#define ERROR(...) __android_log_print(ANDROID_LOG_ERROR, kTag, __VA_ARGS__)

using HookFn = int (*)(void *, void *, void **);
using UnhookFn = int (*)(void *);
using LibraryCallback = void (*)(const char *, void *);
struct NativeApi {
    uint32_t version;
    HookFn hook;
    UnhookFn unhook;
};

struct RustBuffer {
    uint8_t *data;
    size_t len;
};

struct RustResult {
    int action;
    RustBuffer message;
    RustBuffer injection;
};

extern "C" int majmax_modder_init(const char *configDir);
extern "C" int majmax_modder_process(uintptr_t connection, bool fromClient,
        const uint8_t *data, size_t len, RustResult *result);
extern "C" void majmax_modder_forget_connection(uintptr_t connection);
extern "C" void majmax_capture_configure();
extern "C" void majmax_capture_set_endpoint(uint32_t port, const uint8_t *token, size_t tokenLen);
extern "C" void majmax_modder_free(uint8_t *data, size_t len);
extern "C" int majmax_modder_get_settings(RustBuffer *output);
extern "C" int majmax_modder_update_settings(const uint8_t *patch, size_t patchLen,
        RustBuffer *output);

HookFn installHook = nullptr;
std::atomic<void *> il2cppHandle{nullptr};
std::atomic<bool> bootstrapStarted{false};
std::atomic<bool> toluaLoaded{false};
std::atomic<bool> hooksReady{false};
std::atomic<void *> luaBridgeState{nullptr};
std::atomic<void *> toluaHandle{nullptr};
std::atomic<void *> luaClientInstance{nullptr};
std::atomic<bool> uiPageReady{false};
std::atomic<int64_t> uiScriptLastRunMs{0};
std::mutex uiScriptMutex;
std::string uiScript;

struct Il2CppApi {
    void *(*domainGet)();
    const void **(*assemblies)(void *, size_t *);
    void *(*assemblyImage)(const void *);
    const char *(*imageName)(const void *);
    void *(*classFromName)(const void *, const char *, const char *);
    const void *(*classMethods)(void *, void **);
    const char *(*methodName)(const void *);
    uint32_t (*paramCount)(const void *);
    const void *(*paramType)(const void *, uint32_t);
    const void *(*returnType)(const void *);
    bool (*methodIsInstance)(const void *);
    char *(*typeName)(const void *);
    void (*freeMemory)(void *);
    void *(*threadAttach)(void *);
    void (*threadDetach)(void *);
    void *(*stringNew)(const char *);
    uintptr_t (*arrayLength)(void *);
    uint32_t (*arrayHeaderSize)();
    const void *(*getCorlib)();
    void *(*arrayNew)(void *, uintptr_t);
} api{};

void *byteClass = nullptr;

// The game uses ToLua's Lua 5.1-compatible C ABI. We only use exported
// primitives and keep the bridge intentionally small: settings are exchanged
// as UTF-8 JSON strings, so no game-specific Lua table layout is assumed.
using LuaLoadBufferX = int (*)(void *, const char *, size_t, const char *, const char *);
using LuaPushCClosure = void (*)(void *, int (*)(void *), int);
using LuaSetField = void (*)(void *, int, const char *);
using LuaGetTop = int (*)(void *);
using LuaToLString = const char *(*)(void *, int, size_t *);
using LuaPushLString = const char *(*)(void *, const char *, size_t);

LuaLoadBufferX originalLuaLoadBufferX = nullptr;
LuaPushCClosure luaPushCClosure = nullptr;
LuaSetField luaSetField = nullptr;
LuaGetTop luaGetTop = nullptr;
LuaToLString luaToLString = nullptr;
LuaPushLString luaPushLString = nullptr;

using LuaClientOnLoadFinished = void (*)(void *);
using LuaClientGetMainState = void *(*)(void *);
using LuaStateDoString = void (*)(void *, void *, void *);
using LuaClientGetInstance = void *(*)(void *);
using LuaLooperUpdate = void (*)(void *);
LuaClientOnLoadFinished originalLuaClientOnLoadFinished = nullptr;
LuaClientGetMainState luaClientGetMainState = nullptr;
LuaStateDoString luaStateDoString = nullptr;
LuaClientGetInstance luaClientGetInstance = nullptr;
LuaLooperUpdate originalLuaLooperUpdate = nullptr;

void pushLuaText(void *state, const std::string &text) {
    if (luaPushLString) luaPushLString(state, text.data(), text.size());
}

int luaGetModSettings(void *state) {
    RustBuffer output{};
    const int status = majmax_modder_get_settings(&output);
    if (status != 0 || !output.data) {
        pushLuaText(state, "{\"error\":\"settings_unavailable\"}");
        return 1;
    }
    pushLuaText(state, std::string(reinterpret_cast<const char *>(output.data), output.len));
    majmax_modder_free(output.data, output.len);
    return 1;
}

int luaUpdateModSettings(void *state) {
    if (!luaGetTop || !luaToLString || luaGetTop(state) < 1) {
        pushLuaText(state, "{\"error\":\"patch_missing\"}");
        return 1;
    }
    size_t length = 0;
    const char *patch = luaToLString(state, 1, &length);
    if (!patch || length == 0 || length > 64 * 1024) {
        pushLuaText(state, "{\"error\":\"patch_invalid\"}");
        return 1;
    }
    RustBuffer output{};
    const int status = majmax_modder_update_settings(
            reinterpret_cast<const uint8_t *>(patch), length, &output);
    if (status != 0 || !output.data) {
        pushLuaText(state, "{\"error\":\"patch_rejected\"}");
        return 1;
    }
    pushLuaText(state, std::string(reinterpret_cast<const char *>(output.data), output.len));
    majmax_modder_free(output.data, output.len);
    return 1;
}

int luaMarkUiReady(void *) {
    uiPageReady.store(true);
    return 0;
}

int luaLogMessage(void *state) {
    if (!luaGetTop || !luaToLString || luaGetTop(state) < 1) return 0;
    size_t length = 0;
    const char *message = luaToLString(state, 1, &length);
    if (!message || length == 0 || length > 2048) return 0;
    std::string text(message, length);
    INFO("Lua UI: %s", text.c_str());
    return 0;
}

bool installLuaBridge(void *state) {
    if (!state || !luaPushCClosure || !luaSetField || !luaPushLString) return false;
    // Lua 5.1/LuaJIT's LUA_GLOBALSINDEX. The game exports lua_setfield but
    // not the macro-backed lua_setglobal symbol.
    constexpr int kLuaGlobalsIndex = -10002;
    luaPushCClosure(state, luaGetModSettings, 0);
    luaSetField(state, kLuaGlobalsIndex, "__majmax_get_settings");
    luaPushCClosure(state, luaUpdateModSettings, 0);
    luaSetField(state, kLuaGlobalsIndex, "__majmax_update_settings");
    luaPushCClosure(state, luaMarkUiReady, 0);
    luaSetField(state, kLuaGlobalsIndex, "__majmax_ui_ready");
    luaPushCClosure(state, luaLogMessage, 0);
    luaSetField(state, kLuaGlobalsIndex, "__majmax_ui_log");
    INFO("Lua settings bridge registered");
    return true;
}

int hookedLuaLoadBufferX(void *state, const char *buffer, size_t size,
        const char *chunkName, const char *mode) {
    if (luaBridgeState.load() != state && installLuaBridge(state)) {
        luaBridgeState.store(state);
    }
    if (chunkName && (std::strstr(chunkName, "UI") || std::strstr(chunkName, "Setting")
            || std::strstr(chunkName, "setting") || std::strstr(chunkName, "Option"))) {
        INFO("Lua UI chunk: %s (%zu bytes)", chunkName, size);
    }
    return originalLuaLoadBufferX
            ? originalLuaLoadBufferX(state, buffer, size, chunkName, mode)
            : 0;
}

bool installLuaLoadHook(void *handle) {
    if (!handle || !installHook || originalLuaLoadBufferX) return originalLuaLoadBufferX != nullptr;
    auto loadBuffer = reinterpret_cast<LuaLoadBufferX>(dlsym(handle, "luaL_loadbufferx"));
    luaPushCClosure = reinterpret_cast<LuaPushCClosure>(dlsym(handle, "lua_pushcclosure"));
    luaSetField = reinterpret_cast<LuaSetField>(dlsym(handle, "lua_setfield"));
    luaGetTop = reinterpret_cast<LuaGetTop>(dlsym(handle, "lua_gettop"));
    luaToLString = reinterpret_cast<LuaToLString>(dlsym(handle, "lua_tolstring"));
    luaPushLString = reinterpret_cast<LuaPushLString>(dlsym(handle, "lua_pushlstring"));
    if (!loadBuffer || !luaPushCClosure || !luaSetField || !luaGetTop
            || !luaToLString || !luaPushLString) {
        ERROR("ToLua settings bridge exports are incomplete");
        return false;
    }
    int result = installHook(reinterpret_cast<void *>(loadBuffer),
            reinterpret_cast<void *>(hookedLuaLoadBufferX),
            reinterpret_cast<void **>(&originalLuaLoadBufferX));
    INFO("HOOK luaL_loadbufferx result=%d", result);
    if (result != 0) {
        originalLuaLoadBufferX = nullptr;
        return false;
    }
    INFO("ToLua settings bridge hook installed");
    return true;
}

void runUiBootstrap(void *client) {
    if (!client || !luaClientGetMainState || !luaStateDoString || !api.stringNew) return;
    if (uiPageReady.load()) return;
    const int64_t now = std::chrono::duration_cast<std::chrono::milliseconds>(
            std::chrono::steady_clock::now().time_since_epoch()).count();
    const int64_t last = uiScriptLastRunMs.load();
    if (last != 0 && now - last < 500) return;
    uiScriptLastRunMs.store(now);
    void *stateObject = luaClientGetMainState(client);
    if (!stateObject) return;
    std::string script;
    {
        std::lock_guard<std::mutex> lock(uiScriptMutex);
        script = uiScript;
    }
    if (script.empty()) return;
    void *chunk = api.stringNew(script.c_str());
    void *chunkName = api.stringNew("@MajsoulMaxSettings");
    if (!chunk || !chunkName) {
        WARN("Cannot allocate UI bootstrap Lua strings");
        return;
    }
    // OnLoadFinished runs on Unity's main thread. Calling LuaState.DoString here
    // keeps Unity object creation out of the library-loader and WebSocket
    // callback threads. The Lua asset wraps discovery and UI work in pcall.
    luaStateDoString(stateObject, chunk, chunkName);
    INFO("MajsoulMax UI bootstrap executed");
}

void hookedLuaClientOnLoadFinished(void *client) {
    if (originalLuaClientOnLoadFinished) originalLuaClientOnLoadFinished(client);
    luaClientInstance.store(client);
    INFO("LuaClient.OnLoadFinished intercepted");
    runUiBootstrap(client);
}

void hookedLuaLooperUpdate(void *looper) {
    if (originalLuaLooperUpdate) originalLuaLooperUpdate(looper);
    if (!luaClientInstance.load() && luaClientGetInstance) {
        luaClientInstance.store(luaClientGetInstance(nullptr));
    }
    runUiBootstrap(luaClientInstance.load());
}

bool resolveApi(void *handle) {
#define RESOLVE(member, symbol) \
    api.member = reinterpret_cast<decltype(api.member)>(dlsym(handle, symbol)); \
    if (!api.member) { ERROR("Missing IL2CPP export: %s", symbol); return false; }
    RESOLVE(domainGet, "il2cpp_domain_get")
    RESOLVE(assemblies, "il2cpp_domain_get_assemblies")
    RESOLVE(assemblyImage, "il2cpp_assembly_get_image")
    RESOLVE(imageName, "il2cpp_image_get_name")
    RESOLVE(classFromName, "il2cpp_class_from_name")
    RESOLVE(classMethods, "il2cpp_class_get_methods")
    RESOLVE(methodName, "il2cpp_method_get_name")
    RESOLVE(paramCount, "il2cpp_method_get_param_count")
    RESOLVE(paramType, "il2cpp_method_get_param")
    RESOLVE(returnType, "il2cpp_method_get_return_type")
    RESOLVE(methodIsInstance, "il2cpp_method_is_instance")
    RESOLVE(typeName, "il2cpp_type_get_name")
    RESOLVE(freeMemory, "il2cpp_free")
    RESOLVE(threadAttach, "il2cpp_thread_attach")
    RESOLVE(threadDetach, "il2cpp_thread_detach")
    RESOLVE(stringNew, "il2cpp_string_new")
    RESOLVE(arrayLength, "il2cpp_array_length")
    RESOLVE(arrayHeaderSize, "il2cpp_array_object_header_size")
    RESOLVE(getCorlib, "il2cpp_get_corlib")
    RESOLVE(arrayNew, "il2cpp_array_new")
#undef RESOLVE
    return true;
}

bool typeMatches(const void *type, const char *expected) {
    char *name = api.typeName(type);
    bool equal = name && std::strcmp(name, expected) == 0;
    if (name) api.freeMemory(name);
    return equal;
}

bool methodAddress(const void *method, void **address) {
    if (!method || !address) return false;
    std::memcpy(address, method, sizeof(*address));
    Dl_info info{};
    return *address && dladdr(*address, &info) && info.dli_fname
            && std::strstr(info.dli_fname, "libil2cpp.so");
}

bool hookMethod(const void *method, void *replacement, void **original, const char *label) {
    void *address = nullptr;
    if (!methodAddress(method, &address)) {
        WARN("Rejected method pointer for %s", label);
        return false;
    }
    int result = installHook(address, replacement, original);
    INFO("HOOK %s result=%d", label, result);
    return result == 0;
}

bool readArray(void *array, const uint8_t **data, size_t *size) {
    if (!array || !data || !size) return false;
    uintptr_t length = api.arrayLength(array);
    uint32_t header = api.arrayHeaderSize();
    if (length == 0 || length > 16 * 1024 * 1024 || header < 16 || header > 128) return false;
    *size = static_cast<size_t>(length);
    *data = static_cast<const uint8_t *>(array) + header;
    return true;
}

void *newByteArray(const uint8_t *data, size_t size) {
    if (!data || size == 0 || size > 16 * 1024 * 1024 || !byteClass) return nullptr;
    void *array = api.arrayNew(byteClass, size);
    if (!array) return nullptr;
    std::memcpy(static_cast<uint8_t *>(array) + api.arrayHeaderSize(), data, size);
    return array;
}

struct Processed {
    int action = 0;
    void *message = nullptr;
    std::vector<uint8_t> injection;
};

Processed process(void *socket, bool fromClient, void *array) {
    Processed processed;
    if (!hooksReady.load()) return processed;
    const uint8_t *data = nullptr;
    size_t size = 0;
    if (!readArray(array, &data, &size)) return processed;

    RustResult result{};
    int status = majmax_modder_process(reinterpret_cast<uintptr_t>(socket), fromClient,
            data, size, &result);
    if (status != 0) return processed;
    processed.action = result.action;
    if (result.action == 2 && result.message.data && result.message.len) {
        processed.message = newByteArray(result.message.data, result.message.len);
        if (!processed.message) {
            WARN("Cannot allocate replacement byte array; passing original message");
            processed.action = 0;
        } else {
            INFO("%s message replaced: %zu -> %zu bytes", fromClient ? "OUT" : "IN",
                    size, result.message.len);
        }
    }
    if (result.injection.data && result.injection.len) {
        processed.injection.assign(result.injection.data,
                result.injection.data + result.injection.len);
    }
    majmax_modder_free(result.message.data, result.message.len);
    majmax_modder_free(result.injection.data, result.injection.len);
    return processed;
}

std::mutex pendingMutex;
std::unordered_map<void *, std::vector<std::vector<uint8_t>>> pendingInjections;

void queueInjection(void *socket, std::vector<uint8_t> injection) {
    if (injection.empty()) return;
    std::lock_guard<std::mutex> lock(pendingMutex);
    pendingInjections[socket].push_back(std::move(injection));
}

std::vector<std::vector<uint8_t>> takeInjections(void *socket) {
    std::lock_guard<std::mutex> lock(pendingMutex);
    auto iterator = pendingInjections.find(socket);
    if (iterator == pendingInjections.end()) return {};
    auto messages = std::move(iterator->second);
    pendingInjections.erase(iterator);
    return messages;
}

void forgetConnection(void *socket) {
    majmax_modder_forget_connection(reinterpret_cast<uintptr_t>(socket));
    std::lock_guard<std::mutex> lock(pendingMutex);
    pendingInjections.erase(socket);
}

bool installLuaLifecycleHook(void *image) {
    if (!image || !installHook || originalLuaClientOnLoadFinished) {
        return originalLuaClientOnLoadFinished != nullptr;
    }
    void *luaClient = api.classFromName(image, "", "LuaClient");
    void *luaState = api.classFromName(image, "LuaInterface", "LuaState");
    void *luaLooper = api.classFromName(image, "", "LuaLooper");
    INFO("Lua metadata classes client=%p state=%p looper=%p", luaClient, luaState, luaLooper);
    if (!luaClient || !luaState) {
        WARN("LuaClient/LuaState metadata is unavailable; UI bootstrap disabled");
        return false;
    }

    const void *onLoadFinished = nullptr;
    const void *getMainState = nullptr;
    const void *getInstance = nullptr;
    const void *doString = nullptr;
    const void *looperUpdate = nullptr;
    void *iterator = nullptr;
    while (const void *method = api.classMethods(luaClient, &iterator)) {
        const char *name = api.methodName(method);
        if (!name || api.paramCount(method) != 0) continue;
        if (std::strcmp(name, "OnLoadFinished") == 0
                && api.methodIsInstance(method)
                && typeMatches(api.returnType(method), "System.Void")) {
            onLoadFinished = method;
        }
        if (std::strcmp(name, "GetMainState") == 0
                && !typeMatches(api.returnType(method), "System.Void")) {
            getMainState = method;
        }
        if (std::strcmp(name, "get_Instance") == 0
                && !typeMatches(api.returnType(method), "System.Void")) {
            getInstance = method;
        }
    }
    iterator = nullptr;
    while (const void *method = api.classMethods(luaState, &iterator)) {
        const char *name = api.methodName(method);
        if (!name || !api.methodIsInstance(method)
                || !typeMatches(api.returnType(method), "System.Void")
                || api.paramCount(method) != 2) continue;
        if (std::strcmp(name, "DoString") != 0
                || !typeMatches(api.paramType(method, 0), "System.String")
                || !typeMatches(api.paramType(method, 1), "System.String")) continue;
        doString = method;
        break;
    }
    if (luaLooper) {
        iterator = nullptr;
        while (const void *method = api.classMethods(luaLooper, &iterator)) {
            const char *name = api.methodName(method);
            if (name && std::strcmp(name, "Update") == 0 && api.methodIsInstance(method)
                    && api.paramCount(method) == 0
                    && typeMatches(api.returnType(method), "System.Void")) {
                looperUpdate = method;
                break;
            }
        }
    }
    if (!onLoadFinished || !getMainState || !doString
            || !methodAddress(getMainState,
                    reinterpret_cast<void **>(&luaClientGetMainState))
            || !methodAddress(doString, reinterpret_cast<void **>(&luaStateDoString))) {
        WARN("Lua lifecycle method signatures are unavailable; UI bootstrap disabled");
        luaClientGetMainState = nullptr;
        luaStateDoString = nullptr;
        WARN("Lua lifecycle methods found onLoad=%p getMain=%p doString=%p looper=%p",
                onLoadFinished, getMainState, doString, looperUpdate);
        return false;
    }
    if (getInstance) {
        methodAddress(getInstance, reinterpret_cast<void **>(&luaClientGetInstance));
    }
    INFO("Lua lifecycle methods resolved getMain=%p doString=%p getInstance=%p",
            reinterpret_cast<void *>(luaClientGetMainState),
            reinterpret_cast<void *>(luaStateDoString),
            reinterpret_cast<void *>(luaClientGetInstance));
    if (!hookMethod(onLoadFinished,
            reinterpret_cast<void *>(hookedLuaClientOnLoadFinished),
            reinterpret_cast<void **>(&originalLuaClientOnLoadFinished),
            "LuaClient.OnLoadFinished()")) {
        luaClientGetMainState = nullptr;
        luaStateDoString = nullptr;
        return false;
    }
    if (looperUpdate && hookMethod(looperUpdate,
            reinterpret_cast<void *>(hookedLuaLooperUpdate),
            reinterpret_cast<void **>(&originalLuaLooperUpdate),
            "LuaLooper.Update()")) {
        INFO("LuaLooper polling hook installed");
    } else {
        WARN("LuaLooper.Update hook unavailable; UI bootstrap runs once");
    }
    INFO("Lua UI lifecycle hook installed");
    return true;
}

using Send = void (*)(void *, void *, const void *);
using BinaryCallback = void (*)(void *, void *, void *, const void *);
using Close = void (*)(void *, const void *);
Send originalBestSend = nullptr;
BinaryCallback originalBestBinary = nullptr;
Close originalBestClose = nullptr;
Close originalBestDispose = nullptr;

void hookedBestSend(void *socket, void *data, const void *method) {
    Processed processed = process(socket, true, data);
    queueInjection(socket, std::move(processed.injection));
    if (processed.action == 1) {
        INFO("OUT message dropped by Modder");
        return;
    }
    originalBestSend(socket, processed.message ? processed.message : data, method);
}

void hookedBestBinary(void *socket, void *response, void *data, const void *method) {
    Processed processed = process(socket, false, data);
    if (processed.action != 1) {
        originalBestBinary(socket, response, processed.message ? processed.message : data, method);
    } else {
        INFO("IN message dropped by Modder");
    }
    for (const auto &injection : takeInjections(socket)) {
        void *array = newByteArray(injection.data(), injection.size());
        if (array) {
            INFO("IN local notification injected: %zu bytes", injection.size());
            originalBestBinary(socket, response, array, method);
        }
    }
}

void hookedBestClose(void *socket, const void *method) {
    forgetConnection(socket);
    originalBestClose(socket, method);
}

void hookedBestDispose(void *socket, const void *method) {
    forgetConnection(socket);
    originalBestDispose(socket, method);
}

void *bootstrap(void *) {
    void *handle = il2cppHandle.load();
    if (!handle || !resolveApi(handle)) return nullptr;
    for (int attempt = 0; attempt < 150; ++attempt) {
        usleep(200000);
        if (!toluaLoaded.load()) continue;
        void *domain = api.domainGet();
        if (!domain) continue;
        size_t assemblyCount = 0;
        const void **assemblies = api.assemblies(domain, &assemblyCount);
        if (!assemblies || assemblyCount == 0 || assemblyCount > 4096) continue;
        void *image = nullptr;
        for (size_t index = 0; index < assemblyCount; ++index) {
            void *candidate = api.assemblyImage(assemblies[index]);
            const char *name = candidate ? api.imageName(candidate) : nullptr;
            if (name && std::strcmp(name, "Assembly-CSharp.dll") == 0) {
                image = candidate;
                break;
            }
        }
        if (!image) continue;

        void *thread = api.threadAttach(domain);
        if (!thread) {
            ERROR("Cannot attach IL2CPP bootstrap thread");
            return nullptr;
        }
        byteClass = api.classFromName(api.getCorlib(), "System", "Byte");
        void *webSocket = api.classFromName(image, "BestHTTP.WebSocket", "WebSocket");
        if (!byteClass || !webSocket) {
            ERROR("Expected IL2CPP types are missing; hooks disabled");
            api.threadDetach(thread);
            return nullptr;
        }

        if (!installLuaLifecycleHook(image)) {
            WARN("Lua UI lifecycle hook unavailable; traffic hooks continue");
        }

        unsigned messageHooks = 0;
        void *iterator = nullptr;
        while (const void *method = api.classMethods(webSocket, &iterator)) {
            const char *name = api.methodName(method);
            if (!name || !api.methodIsInstance(method)
                    || !typeMatches(api.returnType(method), "System.Void")) continue;
            uint32_t parameters = api.paramCount(method);
            if (std::strcmp(name, "Send") == 0 && parameters == 1
                    && typeMatches(api.paramType(method, 0), "System.Byte[]")) {
                messageHooks += hookMethod(method, reinterpret_cast<void *>(hookedBestSend),
                        reinterpret_cast<void **>(&originalBestSend),
                        "BestHTTP.WebSocket.WebSocket.Send(byte[])");
            } else if (std::strstr(name, "<OnInternalRequestUpgraded>b__") && parameters == 2
                    && typeMatches(api.paramType(method, 0), "BestHTTP.WebSocket.WebSocketResponse")
                    && typeMatches(api.paramType(method, 1), "System.Byte[]")) {
                messageHooks += hookMethod(method, reinterpret_cast<void *>(hookedBestBinary),
                        reinterpret_cast<void **>(&originalBestBinary),
                        "BestHTTP.WebSocket.WebSocket binary callback");
            } else if (std::strcmp(name, "Close") == 0 && parameters == 0) {
                hookMethod(method, reinterpret_cast<void *>(hookedBestClose),
                        reinterpret_cast<void **>(&originalBestClose),
                        "BestHTTP.WebSocket.WebSocket.Close()");
            } else if (std::strcmp(name, "Dispose") == 0 && parameters == 0) {
                hookMethod(method, reinterpret_cast<void *>(hookedBestDispose),
                        reinterpret_cast<void **>(&originalBestDispose),
                        "BestHTTP.WebSocket.WebSocket.Dispose()");
            }
        }
        hooksReady.store(messageHooks == 2);
        if (messageHooks == 2) {
            INFO("BestHTTP Modder ready; array header=%u", api.arrayHeaderSize());
        } else {
            ERROR("BestHTTP signature mismatch: installed %u/2 message hooks", messageHooks);
        }
        api.threadDetach(thread);
        return nullptr;
    }
    ERROR("IL2CPP metadata readiness timed out");
    return nullptr;
}

void onLibraryLoaded(const char *name, void *handle) {
    if (!name || !handle) return;
    if (std::strstr(name, "libil2cpp.so")) {
        il2cppHandle.store(handle);
        if (!bootstrapStarted.exchange(true)) {
            pthread_t thread;
            int result = pthread_create(&thread, nullptr, bootstrap, nullptr);
            if (result == 0) pthread_detach(thread);
            else ERROR("Cannot start bootstrap thread: %d", result);
        }
    } else if (std::strstr(name, "libtolua.so")) {
        toluaHandle.store(handle);
        toluaLoaded.store(true);
        if (!installLuaLoadHook(handle)) {
            WARN("ToLua settings bridge is unavailable; UI injection disabled");
        }
    }
}
} // namespace

static bool loadUiScript(const char *configDir) {
    if (!configDir || !*configDir) return false;
    std::string path(configDir);
    path += "/ui/MajsoulMaxSettings.lua";
    std::ifstream input(path, std::ios::binary | std::ios::ate);
    if (!input) return false;
    const std::streamoff length = input.tellg();
    if (length <= 0 || length > 512 * 1024) return false;
    input.seekg(0, std::ios::beg);
    std::string script(static_cast<size_t>(length), '\0');
    input.read(script.data(), length);
    if (!input) return false;
    {
        std::lock_guard<std::mutex> lock(uiScriptMutex);
        uiScript = std::move(script);
    }
    INFO("Loaded UI bootstrap script (%lld bytes)", static_cast<long long>(length));
    return true;
}

extern "C" JNIEXPORT jint JNICALL
Java_com_yefeng_majmax_hookprobe_ProbeModule_nativeConfigure(
        JNIEnv *environment, jclass, jstring configDir) {
    if (!configDir) return -1;
    const char *path = environment->GetStringUTFChars(configDir, nullptr);
    if (!path) return -2;
    if (!loadUiScript(path)) {
        WARN("UI bootstrap script is missing; settings bridge remains available");
    }
    int result = majmax_modder_init(path);
    majmax_capture_configure();
    environment->ReleaseStringUTFChars(configDir, path);
    INFO("Configuration result=%d hooksReady=%s", result, hooksReady.load() ? "true" : "false");
    return result;
}

extern "C" JNIEXPORT void JNICALL
Java_com_yefeng_majmax_hookprobe_ProbeModule_nativeCaptureEndpoint(
        JNIEnv *environment, jclass, jint port, jbyteArray token) {
    if (port <= 0 || !token || environment->GetArrayLength(token) != 32) {
        majmax_capture_set_endpoint(0, nullptr, 0);
        return;
    }
    jbyte *bytes = environment->GetByteArrayElements(token, nullptr);
    if (!bytes) {
        majmax_capture_set_endpoint(0, nullptr, 0);
        return;
    }
    majmax_capture_set_endpoint(static_cast<uint32_t>(port),
            reinterpret_cast<const uint8_t *>(bytes), 32);
    environment->ReleaseByteArrayElements(token, bytes, JNI_ABORT);
}

extern "C" __attribute__((visibility("default"), used))
LibraryCallback native_init(const NativeApi *entries) {
    if (!entries || !entries->hook) return nullptr;
    installHook = entries->hook;
    INFO("Native API initialized, version=%u", entries->version);
    return onLibraryLoaded;
}
