//! # RDM Panel Plugin API
//!
//! This crate defines the ABI contract between `rdm-panel` and external panel
//! plugins (compiled as `cdylib` shared libraries).
//!
//! ## Plugin contract
//!
//! Each plugin `.so` must export these four C-ABI symbols:
//!
//! ```rust,ignore
//! #[no_mangle]
//! pub extern "C-unwind" fn rdm_plugin_info() -> RdmPluginInfo { ... }
//!
//! #[no_mangle]
//! pub extern "C-unwind" fn rdm_plugin_new_instance(config_toml: *const std::ffi::c_char) -> *mut gtk4::ffi::GtkWidget {
//!     // REQUIRED: tell this .so's gtk4-rs copy that GTK is already initialised.
//!     unsafe { gtk4::set_initialized(); }
//!     ...
//! }
//!
//! #[no_mangle]
//! pub extern "C-unwind" fn rdm_plugin_remove_instances() { ... }
//!
//! #[no_mangle]
//! pub extern "C-unwind" fn rdm_plugin_exit() { ... }
//! ```
//!
//! **Important**: use `extern "C-unwind"` (not `extern "C"`) so that
//! `std::panic::catch_unwind` works inside the function body.
//!
//! **Important**: call `unsafe { gtk4::set_initialized(); }` at the top of
//! `rdm_plugin_new_instance`. Each `.so` links its own copy of the gtk4-rs
//! Rust bindings whose "is GTK initialized?" flag starts false — this call
//! tells it GTK is already running in the host process.
//!
//! Use the [`rdm_export_plugin!`] macro to reduce boilerplate.
//!
//! ## Config
//!
//! `config_toml` is a null-terminated TOML string of the plugin's `[config]`
//! table from `rdm.toml`.  Parse it however you like — `toml::from_str` works
//! well.  It may be null (no config section), in which case use defaults.
//!
//! ## Search paths
//!
//! `rdm-panel` scans for plugins in these directories (in order):
//! - `$HOME/.local/share/rdm/plugins/`
//! - `/usr/local/lib/rdm/plugins/`
//! - `/usr/lib/rdm/plugins/`

use std::ffi::c_char;

/// Basic metadata returned by `rdm_plugin_info`.
///
/// Both pointers must point to `'static` C strings (e.g. string literals via
/// `c"my-plugin".as_ptr()`).
#[repr(C)]
pub struct RdmPluginInfo {
    /// Unique plugin name. Must match the `name` field in `rdm.toml`.
    pub name: *const c_char,
    /// Semver-ish version encoded as `major * 10000 + minor * 100 + patch`.
    pub version: u32,
}

// SAFETY: The pointers inside RdmPluginInfo always point to 'static string
// literals embedded in the plugin binary, so they never dangle.
unsafe impl Send for RdmPluginInfo {}
unsafe impl Sync for RdmPluginInfo {}

/// Symbol names that `rdm-panel` looks for in each `.so`.
pub const SYM_INFO: &[u8] = b"rdm_plugin_info\0";
pub const SYM_NEW_INSTANCE: &[u8] = b"rdm_plugin_new_instance\0";
pub const SYM_REMOVE_INSTANCES: &[u8] = b"rdm_plugin_remove_instances\0";
pub const SYM_EXIT: &[u8] = b"rdm_plugin_exit\0";

/// Convenience macro that generates the four required exported symbols for a
/// plugin.
///
/// # Usage
///
/// ```rust,ignore
/// use rdm_panel_api::rdm_export_plugin;
///
/// struct MyPlugin;
///
/// impl MyPlugin {
///     fn new(config_toml: Option<&str>) -> Self { MyPlugin }
///     fn widget(&self) -> *mut gtk4::ffi::GtkWidget { /* ... */ }
/// }
///
/// rdm_export_plugin!(
///     name:    "my-plugin",
///     version: 1,
///     new:     MyPlugin::new,
///     widget:  MyPlugin::widget,
/// );
/// ```
///
/// The macro manages a `Vec` of instances internally using `thread_local!`
/// storage, which is safe for GTK's single-threaded model.
#[macro_export]
macro_rules! rdm_export_plugin {
    (
        name:    $name:expr,
        version: $ver:expr,
        new:     $new_fn:expr,
        widget:  $widget_fn:expr $(,)?
    ) => {
        use std::cell::RefCell;

        thread_local! {
            static _RDM_INSTANCES: RefCell<Vec<Box<dyn std::any::Any>>> =
                RefCell::new(Vec::new());
        }

        #[no_mangle]
        pub extern "C" fn rdm_plugin_info() -> $crate::RdmPluginInfo {
            $crate::RdmPluginInfo {
                name: concat!($name, "\0").as_ptr() as *const std::ffi::c_char,
                version: $ver,
            }
        }

        #[no_mangle]
        pub extern "C" fn rdm_plugin_new_instance(
            config_toml: *const std::ffi::c_char,
        ) -> *mut gtk4::ffi::GtkWidget {
            let config_str: Option<&str> = if config_toml.is_null() {
                None
            } else {
                unsafe { std::ffi::CStr::from_ptr(config_toml).to_str().ok() }
            };
            let instance = $new_fn(config_str);
            let widget_ptr = $widget_fn(&instance);
            _RDM_INSTANCES.with(|v| v.borrow_mut().push(Box::new(instance)));
            widget_ptr
        }

        #[no_mangle]
        pub extern "C" fn rdm_plugin_remove_instances() {
            _RDM_INSTANCES.with(|v| v.borrow_mut().clear());
        }

        #[no_mangle]
        pub extern "C" fn rdm_plugin_exit() {
            _RDM_INSTANCES.with(|v| v.borrow_mut().clear());
        }
    };
}
