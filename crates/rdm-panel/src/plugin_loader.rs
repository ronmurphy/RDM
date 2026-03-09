use std::ffi::{CStr, CString};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use gtk4::glib;
use libloading::{Library, Symbol};
use rdm_panel_api::{RdmPluginInfo, SYM_EXIT, SYM_INFO, SYM_NEW_INSTANCE, SYM_REMOVE_INSTANCES};

/// A loaded plugin and its resolved function pointers.
struct LoadedPlugin {
    name: String,
    // Keep the Library alive so the symbols remain valid.
    _lib: Library,
    new_instance:
        unsafe extern "C" fn(*const std::ffi::c_char) -> *mut gtk4::ffi::GtkWidget,
    remove_instances: unsafe extern "C" fn(),
    exit: unsafe extern "C" fn(),
}

// SAFETY: the function pointers inside LoadedPlugin come from a loaded shared
// library and are only called from the main GTK thread.
unsafe impl Send for LoadedPlugin {}

static REGISTRY: OnceLock<Mutex<Vec<LoadedPlugin>>> = OnceLock::new();

fn registry() -> std::sync::MutexGuard<'static, Vec<LoadedPlugin>> {
    REGISTRY
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .expect("plugin registry mutex poisoned")
}

/// Search directories scanned in order.  First match wins when a plugin with
/// the same name would appear in multiple locations.
fn search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".local/share/rdm/plugins"));
    }
    paths.push(PathBuf::from("/usr/local/lib/rdm/plugins"));
    paths.push(PathBuf::from("/usr/lib/rdm/plugins"));

    // Dev/local convenience: directory next to the binary.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            paths.push(dir.join("rdm-plugins"));
        }
    }

    paths
}

/// Load all `.so` files found in the search paths.  Plugins whose name is not
/// listed in `wanted` are skipped (pass `None` to load everything found).
pub fn load_plugins(wanted: Option<&[String]>) {
    for dir in search_paths() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(ext) = path.extension() else {
                continue;
            };
            if ext != "so" {
                continue;
            }
            try_load_plugin(&path, wanted);
        }
    }
}

fn try_load_plugin(path: &std::path::Path, wanted: Option<&[String]>) {
    // SAFETY: We hold the Library alive inside LoadedPlugin for as long as
    // the REGISTRY lives, which is the entire process lifetime.
    let lib = match unsafe { Library::new(path) } {
        Ok(l) => l,
        Err(e) => {
            log::warn!("plugin: failed to open {:?}: {}", path, e);
            return;
        }
    };

    // Resolve and call wap_plugin_info to get the name.
    let name: String = unsafe {
        let sym: Result<Symbol<unsafe extern "C" fn() -> RdmPluginInfo>, _> =
            lib.get(SYM_INFO);
        match sym {
            Ok(f) => {
                let info = f();
                if info.name.is_null() {
                    log::warn!("plugin: {:?} returned null name", path);
                    return;
                }
                match CStr::from_ptr(info.name).to_str() {
                    Ok(s) => s.to_owned(),
                    Err(_) => {
                        log::warn!("plugin: {:?} name is not valid UTF-8", path);
                        return;
                    }
                }
            }
            Err(e) => {
                log::warn!("plugin: {:?} missing {}: {}", path, "rdm_plugin_info", e);
                return;
            }
        }
    };

    // Skip if caller only wants a specific set.
    if let Some(wanted) = wanted {
        if !wanted.iter().any(|w| w == &name) {
            return;
        }
    }

    // Skip duplicates (first path wins).
    if registry().iter().any(|p| p.name == name) {
        log::debug!("plugin: '{}' already loaded, skipping {:?}", name, path);
        return;
    }

    // Resolve the other three symbols.
    let new_instance = unsafe {
        let sym: Result<
            Symbol<unsafe extern "C" fn(*const std::ffi::c_char) -> *mut gtk4::ffi::GtkWidget>,
            _,
        > = lib.get(SYM_NEW_INSTANCE);
        match sym {
            Ok(s) => *s,
            Err(e) => {
                log::warn!("plugin '{}': missing rdm_plugin_new_instance: {}", name, e);
                return;
            }
        }
    };

    let remove_instances = unsafe {
        let sym: Result<Symbol<unsafe extern "C" fn()>, _> = lib.get(SYM_REMOVE_INSTANCES);
        match sym {
            Ok(s) => *s,
            Err(e) => {
                log::warn!("plugin '{}': missing rdm_plugin_remove_instances: {}", name, e);
                return;
            }
        }
    };

    let exit_fn = unsafe {
        let sym: Result<Symbol<unsafe extern "C" fn()>, _> = lib.get(SYM_EXIT);
        match sym {
            Ok(s) => *s,
            Err(e) => {
                log::warn!("plugin '{}': missing rdm_plugin_exit: {}", name, e);
                return;
            }
        }
    };

    log::info!("plugin: loaded '{}' from {:?}", name, path);

    registry().push(LoadedPlugin {
        name,
        _lib: lib,
        new_instance,
        remove_instances,
        exit: exit_fn,
    });
}

/// Ask a loaded plugin to create a new GTK4 widget instance.
///
/// `config_toml` is the serialised TOML of the plugin's `[config]` table, or
/// `None` if no config was provided.
///
/// Returns `None` if the plugin isn't loaded or returns a null pointer.
pub fn new_instance(plugin_name: &str, config_toml: Option<&str>) -> Option<gtk4::Widget> {
    // Copy the fn pointer out so we can drop the mutex guard before calling GTK.
    let fn_ptr = {
        let guard = registry();
        let plugin = guard.iter().find(|p| p.name == plugin_name)?;
        plugin.new_instance
    };

    let config_cstr = config_toml.and_then(|s| CString::new(s).ok());
    let config_ptr = config_cstr
        .as_ref()
        .map(|c| c.as_ptr())
        .unwrap_or(std::ptr::null());

    // SAFETY: fn pointer is valid for the process lifetime; called on GTK thread.
    let raw = unsafe { fn_ptr(config_ptr) };
    if raw.is_null() {
        log::warn!("plugin '{}': new_instance returned null", plugin_name);
        return None;
    }

    // SAFETY: the pointer was just returned by GTK4 code compiled against the
    // same GTK4 version; wrapping it is safe.
    let widget = unsafe { glib::translate::from_glib_none(raw) };
    Some(widget)
}

/// Call `rdm_plugin_remove_instances` on every loaded plugin.
pub fn remove_all_instances() {
    let ptrs: Vec<_> = registry().iter().map(|p| p.remove_instances).collect();
    for f in ptrs {
        unsafe { f() };
    }
}

/// Call `rdm_plugin_exit` on every loaded plugin and clear the registry.
pub fn shutdown_all() {
    let ptrs: Vec<_> = registry().iter().map(|p| p.exit).collect();
    for f in ptrs {
        unsafe { f() };
    }
    registry().clear();
}
