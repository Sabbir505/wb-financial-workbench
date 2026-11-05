//! App configuration: `wb.config.json` (next to the exe or CWD), overridable by
//! env vars (WB_API_BASE, WB_MCP_URL, WB_VAULT, WB_DATA, WB_WEBHOOK_PORT, WB_REVIEWER).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppConfig {
    /// I4/I6 read-only base URL, e.g. http://127.0.0.1:8012/api
    pub research_api_base: String,
    /// I3 MCP endpoint, e.g. http://127.0.0.1:8012/mcp
    pub research_mcp_url: String,
    /// I2 vault root (research briefs live under research/{yyyy-mm}/)
    pub vault_root: PathBuf,
    /// Local data dir (audit log, exports)
    pub data_dir: PathBuf,
    /// I1 webhook listener port
    pub webhook_port: u16,
    /// Default reviewer identity for review submissions
    pub reviewer: String,
    /// UI language: "zh" (default) or "en"
    #[serde(default)]
    pub lang: String,
    /// Channel timeout in milliseconds
    pub timeout_ms: u64,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            research_api_base: "http://127.0.0.1:8012/api".into(),
            research_mcp_url: "http://127.0.0.1:8012/mcp".into(),
            vault_root: PathBuf::from("research-vault"),
            data_dir: PathBuf::from("wb-data"),
            webhook_port: 8898,
            reviewer: "张研".into(),
            lang: "zh".into(),
            timeout_ms: 8000,
        }
    }
}

impl AppConfig {
    /// Load config file + env overrides; falls back to defaults for anything absent.
    pub fn load() -> AppConfig {
        let mut cfg = AppConfig::default();
        let candidates = [
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("wb.config.json"))),
            std::env::current_dir()
                .ok()
                .map(|d| d.join("wb.config.json")),
        ];
        if let Some(Some(path)) = candidates
            .iter()
            .find(|c| c.is_some() && c.as_ref().unwrap().exists())
        {
            if let Ok(text) = std::fs::read_to_string(path) {
                if let Ok(file_cfg) = serde_json::from_str::<AppConfig>(&text) {
                    cfg = file_cfg;
                }
            }
        }
        if let Ok(v) = std::env::var("WB_API_BASE") {
            cfg.research_api_base = v;
        }
        if let Ok(v) = std::env::var("WB_MCP_URL") {
            cfg.research_mcp_url = v;
        }
        if let Ok(v) = std::env::var("WB_VAULT") {
            cfg.vault_root = PathBuf::from(v);
        }
        if let Ok(v) = std::env::var("WB_DATA") {
            cfg.data_dir = PathBuf::from(v);
        }
        if let Ok(v) = std::env::var("WB_WEBHOOK_PORT") {
            if let Ok(p) = v.parse() {
                cfg.webhook_port = p;
            }
        }
        if let Ok(v) = std::env::var("WB_REVIEWER") {
            cfg.reviewer = v;
        }
        if let Ok(v) = std::env::var("WB_LANG") {
            cfg.lang = v;
        }
        cfg
    }

    /// Best-effort persist (cwd wb.config.json) so UI choices survive restarts.
    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            if let Ok(cwd) = std::env::current_dir() {
                let _ = std::fs::write(cwd.join("wb.config.json"), json);
            }
        }
    }
}
