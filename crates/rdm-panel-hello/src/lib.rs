use gtk4::prelude::*;
use rdm_panel_api::RdmPluginInfo;
use std::cell::RefCell;

thread_local! {
    static INSTANCES: RefCell<Vec<gtk4::Button>> = RefCell::new(Vec::new());
}

#[no_mangle]
pub extern "C-unwind" fn rdm_plugin_info() -> RdmPluginInfo {
    RdmPluginInfo {
        name: c"hello".as_ptr(),
        version: 1,
    }
}

#[no_mangle]
pub extern "C-unwind" fn rdm_plugin_new_instance(
    _config_toml: *const std::ffi::c_char,
) -> *mut gtk4::ffi::GtkWidget {
    // GTK is initialised by the host process; tell this .so's copy of gtk4-rs.
    unsafe { gtk4::set_initialized(); }
    let btn = gtk4::Button::with_label("😊");
    btn.add_css_class("tray-btn");
    let raw = btn.upcast_ref::<gtk4::Widget>().as_ptr();
    // Keep btn alive so the raw pointer stays valid.
    INSTANCES.with(|v| v.borrow_mut().push(btn));
    raw
}

#[no_mangle]
pub extern "C-unwind" fn rdm_plugin_remove_instances() {
    INSTANCES.with(|v| v.borrow_mut().clear());
}

#[no_mangle]
pub extern "C-unwind" fn rdm_plugin_exit() {
    INSTANCES.with(|v| v.borrow_mut().clear());
}
