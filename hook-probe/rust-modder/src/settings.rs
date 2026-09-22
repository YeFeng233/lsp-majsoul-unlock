// Reduced from MajsoulMax-rs src/settings.rs at the pinned submodule commit.
// It keeps only the data and methods consumed by the upstream Modder.
use crate::proto::lq::ViewSlot;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
};
use tracing::error;

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct MaxData {
    pub character: Vec<u32>,
    pub skin: Vec<u32>,
    pub title: Vec<u32>,
    pub item: Vec<u32>,
    pub loading_image: Vec<u32>,
    pub emoji: HashMap<u32, Vec<u32>>,
    pub endings: Vec<u32>,
}

impl MaxData {
    pub fn load(dir: &Path) -> Result<Self> {
        let content =
            std::fs::read_to_string(dir.join("max_data.yaml")).context("无法读取 max_data.yaml")?;
        parse_max_data(&content)
    }
}

fn parse_max_data(content: &str) -> Result<MaxData> {
    let mut result = MaxData::default();
    let mut section = String::new();
    let mut emoji_character = None;
    for (line_number, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if !line.starts_with(' ') && !trimmed.starts_with("- ") {
            ensure!(
                trimmed.ends_with(':'),
                "invalid max_data line {}",
                line_number + 1
            );
            section = trimmed.trim_end_matches(':').to_owned();
            emoji_character = None;
            ensure!(
                matches!(
                    section.as_str(),
                    "character" | "skin" | "title" | "item" | "loading_image" | "emoji" | "endings"
                ),
                "unknown max_data section on line {}",
                line_number + 1
            );
            continue;
        }
        if section == "emoji" && line.starts_with("  ") && trimmed.ends_with(':') {
            let id = trimmed
                .trim_end_matches(':')
                .parse::<u32>()
                .with_context(|| format!("invalid emoji character on line {}", line_number + 1))?;
            result.emoji.entry(id).or_default();
            emoji_character = Some(id);
            continue;
        }
        ensure!(
            trimmed.starts_with("- "),
            "invalid max_data line {}",
            line_number + 1
        );
        let id = trimmed[2..]
            .parse::<u32>()
            .with_context(|| format!("invalid data ID on line {}", line_number + 1))?;
        match section.as_str() {
            "character" => result.character.push(id),
            "skin" => result.skin.push(id),
            "title" => result.title.push(id),
            "item" => result.item.push(id),
            "loading_image" => result.loading_image.push(id),
            "endings" => result.endings.push(id),
            "emoji" => result
                .emoji
                .get_mut(&emoji_character.context("emoji item without character")?)
                .expect("emoji character inserted above")
                .push(id),
            _ => bail!(
                "list item outside a max_data section on line {}",
                line_number + 1
            ),
        }
    }
    ensure!(!result.character.is_empty(), "max_data has no characters");
    ensure!(!result.skin.is_empty(), "max_data has no skins");
    ensure!(!result.title.is_empty(), "max_data has no titles");
    ensure!(!result.item.is_empty(), "max_data has no items");
    ensure!(
        !result.loading_image.is_empty(),
        "max_data has no loading images"
    );
    ensure!(!result.emoji.is_empty(), "max_data has no emoji data");
    ensure!(
        result.emoji.values().all(|items| !items.is_empty()),
        "empty emoji list"
    );
    ensure!(!result.endings.is_empty(), "max_data has no endings");
    Ok(result)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct ModSettings {
    /// Whether the message modifier should be enabled after the next process start.
    /// Missing in older settings files, where serde's `default` value keeps the
    /// existing behavior enabled.
    pub enabled: bool,
    pub main_char: u32,
    pub char_skin: HashMap<u32, u32>,
    pub nickname: String,
    pub star_character: Vec<u32>,
    pub hidden_characters: Vec<u32>,
    hint_switch: bool,
    pub title: u32,
    pub loading_bg: Vec<u32>,
    emoji_switch: bool,
    pub views_presets: [Vec<ViewSlot>; 10],
    pub preset_index: u32,
    show_server: bool,
    anti_nickname_censorship: bool,
    pub random_char_switch: bool,
    pub random_char_pool: Vec<(u32, u32)>,
    pub verified: u32,
    #[serde(skip)]
    dir: PathBuf,
}

impl Default for ModSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            main_char: 200001,
            char_skin: HashMap::new(),
            nickname: String::new(),
            star_character: Vec::new(),
            hidden_characters: Vec::new(),
            hint_switch: true,
            title: 0,
            loading_bg: Vec::new(),
            emoji_switch: false,
            views_presets: Default::default(),
            preset_index: 0,
            show_server: true,
            anti_nickname_censorship: true,
            random_char_switch: false,
            random_char_pool: Vec::new(),
            verified: 0,
            dir: PathBuf::new(),
        }
    }
}

/// Fields exposed by the in-game settings page. Keeping this as a patch rather
/// than accepting a whole replacement document prevents a UI update from
/// accidentally deleting settings that are still managed by the original game
/// pages (skins, views and titles).
#[derive(Serialize, Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsPatch {
    pub enabled: Option<bool>,
    pub hint_switch: Option<bool>,
    pub emoji_switch: Option<bool>,
    pub show_server: Option<bool>,
    pub anti_nickname_censorship: Option<bool>,
    pub random_char_switch: Option<bool>,
    pub random_char_pool: Option<Vec<(u32, u32)>>,
    pub nickname: Option<String>,
}

impl SettingsPatch {
    pub fn is_empty(&self) -> bool {
        self.enabled.is_none()
            && self.hint_switch.is_none()
            && self.emoji_switch.is_none()
            && self.show_server.is_none()
            && self.anti_nickname_censorship.is_none()
            && self.random_char_switch.is_none()
            && self.random_char_pool.is_none()
            && self.nickname.is_none()
    }
}

impl ModSettings {
    pub fn load(dir: &Path) -> Result<Self> {
        let file = dir.join("settings.mod.json");
        let mut settings: Self = serde_json::from_str(
            &std::fs::read_to_string(&file).context("无法读取 settings.mod.json")?,
        )
        .context("无法解析 settings.mod.json")?;
        settings.dir = dir.to_path_buf();
        Ok(settings)
    }

    pub fn default_avatar_id(char_id: u32) -> Result<u32> {
        let id = char_id.to_string();
        let suffix = id
            .get(4..)
            .with_context(|| format!("角色 ID {char_id} 过短"))?;
        format!("40{suffix}01")
            .parse()
            .with_context(|| format!("无法解析角色 {char_id} 的默认装扮 ID"))
    }

    pub fn avatar_id_of(&self, char_id: u32) -> Result<u32> {
        self.char_skin
            .get(&char_id)
            .copied()
            .map(Ok)
            .unwrap_or_else(|| Self::default_avatar_id(char_id))
    }

    pub fn main_avatar_id(&self) -> Result<u32> {
        self.avatar_id_of(self.main_char)
    }

    pub fn current_preset(&self) -> &[ViewSlot] {
        self.views_presets
            .get(self.preset_index as usize)
            .unwrap_or(&self.views_presets[0])
    }

    pub fn avatar_frame(&self) -> u32 {
        self.current_preset()
            .iter()
            .find(|view| view.slot == 5)
            .map(|view| view.item_id)
            .unwrap_or_default()
    }

    pub fn preset_count(&self) -> usize {
        self.views_presets.len()
    }

    pub fn hint_on(&self) -> bool {
        self.hint_switch
    }

    pub fn emoji_on(&self) -> bool {
        self.emoji_switch
    }

    pub fn show_server(&self) -> bool {
        self.show_server
    }

    pub fn anti_nickname_censorship(&self) -> bool {
        self.anti_nickname_censorship
    }

    pub fn apply_patch(&mut self, patch: SettingsPatch) -> Result<()> {
        if let Some(nickname) = patch.nickname {
            ensure!(nickname.chars().count() <= 64, "昵称不能超过 64 个字符");
            ensure!(
                nickname.chars().all(|character| !character.is_control()),
                "昵称不能包含控制字符"
            );
            self.nickname = nickname;
        }
        if let Some(pool) = patch.random_char_pool {
            ensure!(pool.len() <= 256, "随机角色池不能超过 256 项");
            ensure!(
                pool.iter().all(|(character, skin)| *character != 0 && *skin != 0),
                "随机角色池包含无效角色"
            );
            self.random_char_pool = pool;
        }
        if let Some(value) = patch.enabled {
            self.enabled = value;
        }
        if let Some(value) = patch.hint_switch {
            self.hint_switch = value;
        }
        if let Some(value) = patch.emoji_switch {
            self.emoji_switch = value;
        }
        if let Some(value) = patch.show_server {
            self.show_server = value;
        }
        if let Some(value) = patch.anti_nickname_censorship {
            self.anti_nickname_censorship = value;
        }
        if let Some(value) = patch.random_char_switch {
            self.random_char_switch = value;
        }
        Ok(())
    }

    pub fn write(&self) {
        if let Err(error) = self.write_atomic() {
            error!("Failed to write settings.mod.json: {error}");
        }
    }

    /// Persist the complete settings document without exposing a partially
    /// written JSON file to the next game process. The temporary file lives
    /// beside the target so rename remains atomic on Android's filesystem.
    pub fn write_atomic(&self) -> Result<()> {
        ensure!(
            !self.dir.as_os_str().is_empty(),
            "设置目录未初始化"
        );
        let path = self.dir.join("settings.mod.json");
        let content = serde_json::to_vec_pretty(self).context("序列化 settings.mod.json 失败")?;
        let temporary = path.with_extension(format!(
            "json.tmp.{}.{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("writer")
        ));
        let _ = std::fs::remove_file(&temporary);
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .with_context(|| format!("创建临时设置文件失败: {}", temporary.display()))?;
        let write_result = (|| -> Result<()> {
            file.write_all(&content).context("写入临时设置文件失败")?;
            file.sync_all().context("同步临时设置文件失败")?;
            std::fs::rename(&temporary, &path).context("替换 settings.mod.json 失败")?;
            Ok(())
        })();
        if write_result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        write_result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_upstream_max_data() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../external/MajsoulMax-rs/liqi_config");
        let data = MaxData::load(&path).unwrap();
        assert!(!data.character.is_empty());
        assert!(!data.endings.is_empty());
    }

    #[test]
    fn missing_enabled_field_keeps_mod_enabled() {
        let settings: ModSettings = serde_json::from_str(
            r#"{"mainChar":200001,"nickname":"旧配置"}"#,
        )
        .unwrap();
        assert!(settings.enabled);
        assert_eq!(settings.nickname, "旧配置");
    }

    #[test]
    fn patch_changes_only_exposed_fields() {
        let mut settings = ModSettings::default();
        settings.main_char = 200123;
        settings.title = 700001;
        settings
            .apply_patch(SettingsPatch {
                hint_switch: Some(false),
                nickname: Some("测试名".to_owned()),
                ..Default::default()
            })
            .unwrap();
        assert!(!settings.hint_on());
        assert_eq!(settings.nickname, "测试名");
        assert_eq!(settings.main_char, 200123);
        assert_eq!(settings.title, 700001);
    }

    #[test]
    fn patch_rejects_control_characters_and_bad_pool() {
        let mut settings = ModSettings::default();
        assert!(settings
            .apply_patch(SettingsPatch {
                nickname: Some("bad\nname".to_owned()),
                ..Default::default()
            })
            .is_err());
        assert!(settings
            .apply_patch(SettingsPatch {
                random_char_pool: Some(vec![(0, 400101)]),
                ..Default::default()
            })
            .is_err());
    }
}
