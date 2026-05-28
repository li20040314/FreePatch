use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default)]
    pub patch_file: String,
    #[serde(default)]
    pub project_path: String,
    #[serde(default = "default_web_content")]
    pub web_content: String,
    #[serde(default)]
    pub class_path: String,
    #[serde(default)]
    pub des_path: String,
    #[serde(default)]
    pub version: String,
    #[serde(default = "default_src_java_prefix")]
    pub src_java_prefix: String,
    #[serde(default = "default_src_resource_prefix")]
    pub src_resource_prefix: String,
    #[serde(default = "default_src_webapp_prefix")]
    pub src_webapp_prefix: String,
}

fn default_web_content() -> String {
    "WebContent".to_string()
}
fn default_src_java_prefix() -> String {
    "src/main/java".to_string()
}
fn default_src_resource_prefix() -> String {
    "src/main/resources".to_string()
}
fn default_src_webapp_prefix() -> String {
    "src/main/webapp".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            patch_file: String::new(),
            project_path: String::new(),
            web_content: default_web_content(),
            class_path: String::new(),
            des_path: String::new(),
            version: chrono::Local::now().format("%Y%m%d-%H%M%S").to_string(),
            src_java_prefix: default_src_java_prefix(),
            src_resource_prefix: default_src_resource_prefix(),
            src_webapp_prefix: default_src_webapp_prefix(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileStore {
    pub current_profile: String,
    pub profiles: HashMap<String, Settings>,
}

impl Default for ProfileStore {
    fn default() -> Self {
        let mut profiles = HashMap::new();
        profiles.insert("默认配置".to_string(), Settings::default());
        Self {
            current_profile: "默认配置".to_string(),
            profiles,
        }
    }
}

pub struct AppConfig {
    pub data: ProfileStore,
}

pub fn load_config(path: &Path) -> ProfileStore {
    let data = match fs::read_to_string(path) {
        Ok(d) => d,
        Err(_) => {
            let ps = ProfileStore::default();
            save_config(path, &ps);
            return ps;
        }
    };

    // 先尝试解析为新格式 ProfileStore
    if let Ok(ps) = serde_json::from_str::<ProfileStore>(&data) {
        if !ps.profiles.is_empty() {
            return ps;
        }
    }

    // 尝试解析为旧格式（单个Settings）
    if let Ok(old) = serde_json::from_str::<Settings>(&data) {
        if !old.patch_file.is_empty() {
            let mut profiles = HashMap::new();
            profiles.insert("默认配置".to_string(), old);
            let ps = ProfileStore {
                current_profile: "默认配置".to_string(),
                profiles,
            };
            save_config(path, &ps);
            return ps;
        }
    }

    let ps = ProfileStore::default();
    save_config(path, &ps);
    ps
}

pub fn save_config(path: &Path, ps: &ProfileStore) {
    if let Ok(data) = serde_json::to_string_pretty(ps) {
        let _ = fs::write(path, data);
    }
}
