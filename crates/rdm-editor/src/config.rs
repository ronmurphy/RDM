use rdm_common::config::{EditorConfig, RdmConfig};

/// Load the editor configuration from rdm.toml, with defaults if missing.
pub fn load() -> EditorConfig {
    RdmConfig::load().editor
}

/// Return the startup directory: configured default_dir → home → /tmp.
pub fn startup_dir(cfg: &EditorConfig) -> std::path::PathBuf {
    if !cfg.default_dir.is_empty() {
        let p = std::path::PathBuf::from(&cfg.default_dir);
        if p.is_dir() {
            return p;
        }
    }
    dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
}
