pub mod config;
pub mod desktop_apps;
pub mod display;
pub mod theme;

pub const APP_NAME: &str = "RDM";
pub const APP_ID: &str = "org.rdm.desktop";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const BUILD_NUMBER: &str = env!("RDM_BUILD_NUMBER");

/// Build timestamp embedded at compile time
pub fn build_version_string() -> String {
    format!("RDM v{}-b{} ({})", VERSION, BUILD_NUMBER, env!("RDM_BUILD_ID"))
}
