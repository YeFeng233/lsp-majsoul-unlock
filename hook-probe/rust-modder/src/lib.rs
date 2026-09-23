//! Synchronous C ABI around the upstream MajsoulMax Rust Modder.

mod proto;
mod settings;
mod capture;

use std::{
    collections::HashMap,
    ffi::CStr,
    os::raw::{c_char, c_int},
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    ptr, slice,
    sync::{Mutex, OnceLock},
};

use anyhow::{Context, Result, bail};
use bytes::Bytes;
use settings::{MaxData, ModSettings, SettingsPatch};
use tokio::{runtime::Runtime, sync::RwLock};

// Keep the upstream source pinned while adding the two accessors required by
// the in-game page in the same Rust module. This preserves access to its
// private RwLock without rebuilding Modder (which would lose its account,
// contract and request state).
mod upstream_modder {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../external/MajsoulMax-rs/src/modder.rs"
    ));

    impl Modder {
        pub async fn settings_snapshot(&self) -> crate::settings::ModSettings {
            self.mod_settings.read().await.clone()
        }

        pub async fn replace_settings(&self, settings: crate::settings::ModSettings) {
            *self.mod_settings.write().await = settings;
        }
    }
}

use upstream_modder::Modder;

const PASS: c_int = 0;
const DROP: c_int = 1;
const REPLACE: c_int = 2;

#[repr(C)]
pub struct OwnedBuffer {
    pub data: *mut u8,
    pub len: usize,
}

impl Default for OwnedBuffer {
    fn default() -> Self {
        Self {
            data: ptr::null_mut(),
            len: 0,
        }
    }
}

#[repr(C)]
#[derive(Default)]
pub struct ProcessResult {
    pub action: c_int,
    pub message: OwnedBuffer,
    pub injection: OwnedBuffer,
}

struct Core {
    runtime: Runtime,
    modder: Modder,
    requests: HashMap<(usize, u16), String>,
    active_enabled: bool,
}

static CORE: OnceLock<Mutex<Core>> = OnceLock::new();

#[cfg(target_os = "android")]
#[link(name = "log")]
unsafe extern "C" {
    fn __android_log_write(priority: c_int, tag: *const c_char, text: *const c_char) -> c_int;
}

#[cfg(target_os = "android")]
fn log_message(priority: c_int, message: &str) {
    use std::ffi::CString;
    let Ok(tag) = CString::new("MajsoulHook") else {
        return;
    };
    let Ok(text) = CString::new(message.replace('\0', "")) else {
        return;
    };
    unsafe {
        __android_log_write(priority, tag.as_ptr(), text.as_ptr());
    }
}

#[cfg(not(target_os = "android"))]
fn log_message(_priority: c_int, message: &str) {
    eprintln!("{message}");
}

fn into_owned(bytes: Bytes) -> OwnedBuffer {
    let mut boxed = bytes.to_vec().into_boxed_slice();
    let result = OwnedBuffer {
        data: boxed.as_mut_ptr(),
        len: boxed.len(),
    };
    std::mem::forget(boxed);
    result
}

fn read_varint(data: &[u8], offset: &mut usize) -> Option<u64> {
    let mut value = 0_u64;
    for shift in (0..70).step_by(7) {
        let byte = *data.get(*offset)?;
        *offset += 1;
        if shift == 63 && byte > 1 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

fn skip_field(wire_type: u8, data: &[u8], offset: &mut usize) -> Option<()> {
    match wire_type {
        0 => {
            read_varint(data, offset)?;
        }
        1 => *offset = offset.checked_add(8)?,
        2 => {
            let len = usize::try_from(read_varint(data, offset)?).ok()?;
            *offset = offset.checked_add(len)?;
        }
        5 => *offset = offset.checked_add(4)?,
        _ => return None,
    }
    (*offset <= data.len()).then_some(())
}

fn envelope_method(data: &[u8]) -> Option<String> {
    let mut offset = 0;
    while offset < data.len() {
        let key = read_varint(data, &mut offset)?;
        let field = key >> 3;
        let wire = (key & 7) as u8;
        if field == 1 && wire == 2 {
            let len = usize::try_from(read_varint(data, &mut offset)?).ok()?;
            let end = offset.checked_add(len)?;
            let method = std::str::from_utf8(data.get(offset..end)?).ok()?;
            if method.starts_with(".lq.")
                && method
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_'))
            {
                return Some(method.to_owned());
            }
            return None;
        }
        skip_field(wire, data, &mut offset)?;
    }
    None
}

impl Core {
    fn new(config_dir: PathBuf) -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("majsoul-modder")
            .enable_all()
            .build()
            .context("创建 Modder runtime 失败")?;
        let settings = ModSettings::load(&config_dir)?;
        let active_enabled = settings.enabled;
        let max_data = MaxData::load(&config_dir)?;
        let modder = runtime.block_on(Modder::new(RwLock::new(settings), max_data))?;
        Ok(Self {
            runtime,
            modder,
            requests: HashMap::new(),
            active_enabled,
        })
    }

    fn process(&mut self, connection: usize, from_client: bool, input: &[u8]) -> ProcessResult {
        if !self.active_enabled {
            return ProcessResult::default();
        }
        let Some(&kind) = input.first() else {
            return ProcessResult::default();
        };
        if (kind == 2) != from_client && (kind == 2 || kind == 3) {
            return ProcessResult::default();
        }

        let method = match kind {
            1 => envelope_method(&input[1..]).unwrap_or_default(),
            2 if input.len() >= 3 => {
                let id = u16::from_le_bytes([input[1], input[2]]);
                let method = envelope_method(&input[3..]).unwrap_or_default();
                if !method.is_empty() {
                    if self.requests.len() >= 4096 {
                        self.requests.clear();
                        log_message(5, "Request map limit reached; stale mappings cleared");
                    }
                    self.requests.insert((connection, id), method.clone());
                }
                method
            }
            3 if input.len() >= 3 => {
                let id = u16::from_le_bytes([input[1], input[2]]);
                self.requests.remove(&(connection, id)).unwrap_or_default()
            }
            _ => return ProcessResult::default(),
        };

        // The Android module exposes its information in the manager UI. Keep
        // announcement traffic byte-for-byte intact instead of adding the
        // upstream Modder's synthetic startup notice. Resolve/remove the RPC
        // mapping first so these responses cannot leave stale request IDs.
        if matches!(
            method.as_str(),
            ".lq.Lobby.fetchAnnouncement"
                | ".lq.Lobby.readAnnouncement"
                | ".lq.NotifyAnnouncementUpdate"
        ) {
            if kind == 3 && method == ".lq.Lobby.fetchAnnouncement" {
                log_message(4, "Official announcements preserved; module notice disabled");
            }
            return ProcessResult::default();
        }

        let outcome = self.runtime.block_on(self.modder.modify(
            Bytes::copy_from_slice(input),
            from_client,
            method,
        ));
        let mut result = ProcessResult::default();
        match outcome.msg {
            None => result.action = DROP,
            Some(message) if message.as_ref() == input => result.action = PASS,
            Some(message) => {
                result.action = REPLACE;
                result.message = into_owned(message);
            }
        }
        if let Some(injection) = outcome.inject_msg {
            result.injection = into_owned(injection);
        }
        result
    }

    fn settings_json(&mut self) -> Result<Vec<u8>> {
        let settings = self.runtime.block_on(self.modder.settings_snapshot());
        let settings_enabled = settings.enabled;
        let value = serde_json::json!({
            "settings": settings,
            "activeEnabled": self.active_enabled,
            "restartRequired": settings_enabled != self.active_enabled,
        });
        serde_json::to_vec(&value).context("序列化设置快照失败")
    }

    fn update_settings(&mut self, patch: SettingsPatch) -> Result<Vec<u8>> {
        if patch.is_empty() {
            bail!("设置补丁不能为空");
        }
        let mut settings = self.runtime.block_on(self.modder.settings_snapshot());
        settings.apply_patch(patch)?;
        settings.write_atomic()?;
        let restart_required = settings.enabled != self.active_enabled;
        self.runtime
            .block_on(self.modder.replace_settings(settings.clone()));
        let value = serde_json::json!({
            "settings": settings,
            "activeEnabled": self.active_enabled,
            "restartRequired": restart_required,
        });
        serde_json::to_vec(&value).context("序列化更新后的设置失败")
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn majmax_modder_init(config_dir: *const c_char) -> c_int {
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<()> {
        if CORE.get().is_some() {
            return Ok(());
        }
        if config_dir.is_null() {
            bail!("配置目录为空");
        }
        let path = unsafe { CStr::from_ptr(config_dir) }
            .to_str()
            .context("配置目录不是 UTF-8")?;
        CORE.set(Mutex::new(Core::new(PathBuf::from(path))?))
            .map_err(|_| anyhow::anyhow!("Modder 已初始化"))?;
        Ok(())
    }));
    match result {
        Ok(Ok(())) => {
            log_message(4, "Rust Modder initialized");
            0
        }
        Ok(Err(error)) => {
            log_message(6, &format!("Rust Modder initialization failed: {error:#}"));
            -1
        }
        Err(_) => {
            log_message(6, "Rust Modder initialization panicked");
            -2
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn majmax_capture_configure() {
    capture::configure();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn majmax_capture_set_endpoint(
    port: u32,
    token: *const u8,
    token_len: usize,
) {
    if token_len != 32 || token.is_null() || port > u16::MAX as u32 {
        capture::set_endpoint(0, &[]);
        return;
    }
    let token = unsafe { std::slice::from_raw_parts(token, token_len) };
    capture::set_endpoint(port as u16, token);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn majmax_modder_process(
    connection: usize,
    from_client: bool,
    data: *const u8,
    len: usize,
    output: *mut ProcessResult,
) -> c_int {
    if output.is_null() || (data.is_null() && len != 0) || len > 16 * 1024 * 1024 {
        return -1;
    }
    unsafe { ptr::write(output, ProcessResult::default()) };
    let Some(core) = CORE.get() else {
        return 1;
    };
    let result = catch_unwind(AssertUnwindSafe(|| {
        let input = if len == 0 {
            &[]
        } else {
            unsafe { slice::from_raw_parts(data, len) }
        };
        // Copy before modification and before the unlock feature's enabled
        // check. Locally injected cosmetic notifications never reach here.
        capture::publish(connection, u8::from(from_client), input);
        core.lock()
            .map_err(|_| ())
            .map(|mut guard| guard.process(connection, from_client, input))
    }));
    match result {
        Ok(Ok(result)) => {
            unsafe { ptr::write(output, result) };
            0
        }
        Ok(Err(())) => {
            log_message(
                6,
                "Rust Modder state lock is poisoned; passing message through",
            );
            -2
        }
        Err(_) => {
            log_message(
                6,
                "Rust Modder processing panicked; passing message through",
            );
            -3
        }
    }
}

/// Return a UTF-8 JSON snapshot. The caller owns the returned buffer and must
/// release it with `majmax_modder_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn majmax_modder_get_settings(
    output: *mut OwnedBuffer,
) -> c_int {
    if output.is_null() {
        return -1;
    }
    unsafe { ptr::write(output, OwnedBuffer::default()) };
    let Some(core) = CORE.get() else {
        return 1;
    };
    let result = catch_unwind(AssertUnwindSafe(|| {
        core.lock()
            .map_err(|_| anyhow::anyhow!("Rust Modder state lock is poisoned"))
            .and_then(|mut guard| guard.settings_json())
    }));
    match result {
        Ok(Ok(bytes)) => {
            let owned = into_owned(Bytes::from(bytes));
            unsafe { ptr::write(output, owned) };
            0
        }
        Ok(Err(error)) => {
            log_message(6, &format!("读取 Mod 设置失败: {error:#}"));
            -2
        }
        Err(_) => {
            log_message(6, "读取 Mod 设置时发生 panic");
            -3
        }
    }
}

/// Apply a camelCase JSON patch and return the resulting snapshot using the
/// same ownership convention as `majmax_modder_get_settings`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn majmax_modder_update_settings(
    patch: *const u8,
    patch_len: usize,
    output: *mut OwnedBuffer,
) -> c_int {
    if output.is_null() || (patch.is_null() && patch_len != 0) || patch_len > 64 * 1024 {
        return -1;
    }
    unsafe { ptr::write(output, OwnedBuffer::default()) };
    let Some(core) = CORE.get() else {
        return 1;
    };
    let input = if patch_len == 0 {
        &[]
    } else {
        unsafe { slice::from_raw_parts(patch, patch_len) }
    };
    let result = catch_unwind(AssertUnwindSafe(|| {
        let parsed: SettingsPatch = serde_json::from_slice(input)
            .context("设置补丁不是有效的 JSON 或包含未知字段")?;
        core.lock()
            .map_err(|_| anyhow::anyhow!("Rust Modder state lock is poisoned"))
            .and_then(|mut guard| guard.update_settings(parsed))
    }));
    match result {
        Ok(Ok(bytes)) => {
            let owned = into_owned(Bytes::from(bytes));
            unsafe { ptr::write(output, owned) };
            0
        }
        Ok(Err(error)) => {
            log_message(6, &format!("更新 Mod 设置失败: {error:#}"));
            -2
        }
        Err(_) => {
            log_message(6, "更新 Mod 设置时发生 panic");
            -3
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn majmax_modder_forget_connection(connection: usize) {
    capture::publish(connection, 2, &[]);
    let Some(core) = CORE.get() else {
        return;
    };
    if let Ok(mut guard) = core.lock() {
        guard
            .requests
            .retain(|(socket, _), _| *socket != connection);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn majmax_modder_free(data: *mut u8, len: usize) {
    if !data.is_null() {
        drop(unsafe { Box::from_raw(ptr::slice_from_raw_parts_mut(data, len)) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{base::BaseMessage, lq};
    use prost::Message;

    #[test]
    fn reads_method_after_an_unknown_field() {
        let mut encoded = bytes::BytesMut::from(&b"\x10\x01\x0a\x13.lq.Lobby.fetchInfo"[..]);
        assert_eq!(
            envelope_method(&encoded),
            Some(".lq.Lobby.fetchInfo".to_owned())
        );
        encoded[4] = b'!';
        assert_eq!(envelope_method(&encoded), None);
    }

    #[test]
    fn rejects_truncated_varints() {
        assert_eq!(envelope_method(&[0x0a, 0x80]), None);
    }

    #[test]
    fn announcement_traffic_preserves_official_messages_and_unknown_fields() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let modder = runtime
            .block_on(Modder::new(
                RwLock::new(ModSettings::default()),
                MaxData::default(),
            ))
            .unwrap();
        let mut core = Core {
            runtime,
            modder,
            requests: HashMap::new(),
            active_enabled: true,
        };
        let frame = |kind, method: &str, payload: Vec<u8>| {
            let mut wire = vec![kind];
            if kind != 1 {
                wire.extend([0x34, 0x12]);
            }
            wire.extend(
                BaseMessage {
                    method_name: method.to_owned(),
                    data: payload,
                }
                .encode_to_vec(),
            );
            // A future envelope field must also survive without re-encoding.
            wire.extend([0xa0, 0x06, 0x01]);
            wire
        };
        let assert_pass = |result: ProcessResult| {
            assert_eq!(result.action, PASS);
            assert!(result.message.data.is_null());
            assert!(result.injection.data.is_null());
        };
        for method in [".lq.Lobby.fetchAnnouncement", ".lq.Lobby.readAnnouncement"] {
            let request = if method.ends_with("readAnnouncement") {
                // Even the old synthetic ID must no longer be converted into
                // an unrelated loginBeat request by the Android adapter.
                lq::ReqReadAnnouncement {
                    announcement_id: 1145141919,
                    ..Default::default()
                }
                .encode_to_vec()
            } else {
                Vec::new()
            };
            assert_pass(core.process(7, true, &frame(2, method, request)));
            assert_eq!(
                core.requests.get(&(7, 0x1234)).map(String::as_str),
                Some(method),
            );
            let mut response = if method.ends_with("readAnnouncement") {
                lq::ResCommon::default().encode_to_vec()
            } else {
                lq::ResAnnouncement {
                    announcements: vec![lq::Announcement {
                        id: 42,
                        title: "Official game update".to_owned(),
                        content: "Keep this announcement".to_owned(),
                        ..Default::default()
                    }],
                    ..Default::default()
                }
                .encode_to_vec()
            };
            response.extend([0xa0, 0x06, 0x01]);
            assert_pass(core.process(7, false, &frame(3, "", response)));
            assert!(core.requests.is_empty());
        }
        assert_pass(core.process(
            7,
            false,
            &frame(1, ".lq.NotifyAnnouncementUpdate", vec![0xa0, 0x06, 0x01]),
        ));
    }

    #[test]
    fn skin_change_produces_fake_request_and_local_notification() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let modder = runtime
            .block_on(Modder::new(
                RwLock::new(ModSettings::default()),
                MaxData::default(),
            ))
            .unwrap();
        let request = lq::ReqChangeCharacterSkin {
            character_id: 200001,
            skin: 400102,
        };
        let envelope = BaseMessage {
            method_name: ".lq.Lobby.changeCharacterSkin".to_owned(),
            data: request.encode_to_vec(),
        };
        let mut wire = vec![2, 0x34, 0x12];
        wire.extend(envelope.encode_to_vec());

        let output = runtime.block_on(modder.modify(Bytes::from(wire), true, ""));
        let replacement = output.msg.expect("the request should be replaced");
        assert_eq!(&replacement[..3], &[2, 0x34, 0x12]);
        assert_eq!(
            BaseMessage::decode(&replacement[3..]).unwrap().method_name,
            ".lq.Lobby.loginBeat"
        );

        let injection = output
            .inject_msg
            .expect("a notification should be injected");
        assert_eq!(injection[0], 1);
        let notification_envelope = BaseMessage::decode(&injection[1..]).unwrap();
        assert_eq!(notification_envelope.method_name, ".lq.NotifyAccountUpdate");
        let notification =
            lq::NotifyAccountUpdate::decode(notification_envelope.data.as_slice()).unwrap();
        let character = &notification.update.unwrap().character.unwrap().characters[0];
        assert_eq!((character.charid, character.skin), (200001, 400102));
    }
}
