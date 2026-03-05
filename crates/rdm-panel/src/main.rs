mod clock;
mod taskbar;
mod toplevel;
mod tray;
mod wifi;

use qmetaobject::prelude::*;
use rdm_common::config::RdmConfig;
use std::cell::RefCell;
use std::collections::HashMap;

// ─── QML UI ──────────────────────────────────────────────────────

/// QML panel — a layer-shell surface anchored to the top (or bottom) edge.
/// Contains: launcher button, taskbar (running windows), clock, system tray.
const PANEL_QML: &str = r#"
import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import QtQuick.Window 2.15
import org.kde.layershell 1.0 as LayerShell

Window {
    id: root
    visible: true
    width: Screen.width
    height: _panel.panelHeight
    color: "transparent"

    LayerShell.Window.scope: "rdm-panel"
    LayerShell.Window.layer: LayerShell.Window.LayerTop
    LayerShell.Window.anchors: _panel.atTop
        ? (LayerShell.Window.AnchorLeft | LayerShell.Window.AnchorRight | LayerShell.Window.AnchorTop)
        : (LayerShell.Window.AnchorLeft | LayerShell.Window.AnchorRight | LayerShell.Window.AnchorBottom)
    LayerShell.Window.exclusionZone: _panel.panelHeight

    Rectangle {
        anchors.fill: parent
        color: "#1a1b26"

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 8
            anchors.rightMargin: 8
            spacing: 0

            // ─── Launcher Button ───────────────────
            Button {
                id: launcherBtn
                text: "  Apps  "
                font.bold: true
                Layout.alignment: Qt.AlignVCenter
                background: Rectangle {
                    color: launcherBtn.hovered ? "#292e42" : "transparent"
                }
                contentItem: Text {
                    text: launcherBtn.text
                    color: "#7aa2f7"
                    font.bold: true
                    font.pixelSize: 13
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
                onClicked: _panel.launchApp("rdm-launcher")
            }

            // ─── Separator ─────────────────────────
            Rectangle {
                width: 1
                Layout.fillHeight: true
                Layout.topMargin: 6
                Layout.bottomMargin: 6
                color: "#3b4261"
            }

            // ─── Taskbar ───────────────────────────
            ListView {
                id: taskbarList
                Layout.fillWidth: true
                Layout.fillHeight: true
                orientation: ListView.Horizontal
                model: _taskbarModel
                spacing: 4
                clip: true
                Layout.leftMargin: 8

                delegate: Button {
                    id: tbBtn
                    height: taskbarList.height
                    width: _panel.taskbarMode === "text"
                        ? Math.min(implicitWidth + 20, 200)
                        : 36

                    background: Rectangle {
                        color: model.isActivated ? "#3d59a1"
                             : tbBtn.hovered ? "#292e42"
                             : "transparent"
                        radius: 4
                        opacity: model.isMinimized ? 0.5 : 1.0
                    }
                    contentItem: Text {
                        text: _panel.taskbarMode === "nerd"
                            ? model.nerdGlyph
                            : _panel.taskbarMode === "icons"
                                ? model.nerdGlyph
                                : model.title
                        color: model.isActivated ? "#ffffff" : "#c0caf5"
                        font.pixelSize: _panel.taskbarMode === "nerd" ? 16 : 13
                        font.family: _panel.taskbarMode === "nerd"
                            ? "JetBrainsMono Nerd Font"
                            : "Inter"
                        elide: Text.ElideRight
                        horizontalAlignment: Text.AlignHCenter
                        verticalAlignment: Text.AlignVCenter
                    }
                    ToolTip.visible: hovered && _panel.taskbarMode !== "text"
                    ToolTip.text: model.title

                    onClicked: _taskbarModel.activateWindow(model.windowId)
                    // Middle-click to close
                    MouseArea {
                        anchors.fill: parent
                        acceptedButtons: Qt.MiddleButton
                        onClicked: _taskbarModel.closeWindow(model.windowId)
                    }
                }
            }

            // ─── Clock ─────────────────────────────
            Text {
                visible: _panel.showClock
                text: _panel.clockText
                color: "#a9b1d6"
                font.pixelSize: 13
                Layout.alignment: Qt.AlignVCenter
                Layout.leftMargin: 12
                Layout.rightMargin: 12
            }

            // ─── System Tray ───────────────────────
            Button {
                id: trayBtn
                Layout.alignment: Qt.AlignVCenter
                font.family: "JetBrainsMono Nerd Font"
                background: Rectangle {
                    color: trayBtn.hovered ? "#292e42" : "transparent"
                }
                contentItem: Text {
                    text: _tray.trayLabel
                    color: _tray.batteryColor
                    font.pixelSize: 13
                    font.family: "JetBrainsMono Nerd Font"
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
                onClicked: trayMenu.open()

                Menu {
                    id: trayMenu
                    width: 240

                    // Battery info
                    MenuItem {
                        text: _tray.batteryMenuLabel
                        enabled: false
                        visible: _tray.batteryPresent
                    }

                    MenuSeparator { visible: _tray.batteryPresent }

                    // WiFi submenu
                    Menu {
                        title: "\uf05a9  WiFi"
                        id: wifiSubMenu

                        Instantiator {
                            model: _wifiModel
                            delegate: MenuItem {
                                text: model.label
                                onTriggered: _wifiModel.connectToNetwork(index)
                            }
                            onObjectAdded: function(index, object) { wifiSubMenu.insertItem(index, object) }
                            onObjectRemoved: function(index, object) { wifiSubMenu.removeItem(object) }
                        }

                        MenuSeparator {}
                        MenuItem {
                            text: "\uf0450  Refresh"
                            onTriggered: _wifiModel.refresh()
                        }
                    }

                    // Session submenu
                    Menu {
                        title: "\uf0425  Session"
                        MenuItem { text: "\uf033e  Lock";     onTriggered: _tray.lock() }
                        MenuItem { text: "\uf0343  Logout";   onTriggered: _tray.logout() }
                        MenuItem { text: "\uf0709  Reboot";   onTriggered: _tray.reboot() }
                        MenuItem { text: "\uf0425  Shutdown"; onTriggered: _tray.shutdown() }
                    }
                }
            }
        }
    }

    // ─── Timers ────────────────────────────────
    Timer {
        interval: 1000
        repeat: true
        running: true
        onTriggered: _panel.updateClock()
    }

    Timer {
        interval: 250
        repeat: true
        running: true
        onTriggered: _taskbarModel.poll()
    }

    Timer {
        interval: 30000
        repeat: true
        running: true
        onTriggered: {
            _tray.updateBattery()
            _wifiModel.refresh()
        }
    }
}
"#;

// ─── Panel Backend ───────────────────────────────────────────────

#[derive(QObject, Default)]
struct PanelBackend {
    base: qt_base_class!(trait QObject),

    panel_height: qt_property!(i32; NOTIFY config_changed),
    at_top: qt_property!(bool; NOTIFY config_changed),
    show_clock: qt_property!(bool; NOTIFY config_changed),
    taskbar_mode: qt_property!(QString; NOTIFY config_changed),
    clock_text: qt_property!(QString; NOTIFY clock_changed),

    config_changed: qt_signal!(),
    clock_changed: qt_signal!(),

    update_clock: qt_method!(fn(&mut self)),
    launch_app: qt_method!(fn(&self, cmd: String)),

    clock_format: String,
}

impl PanelBackend {
    fn from_config(config: &RdmConfig) -> Self {
        let now = chrono::Local::now();
        let clock_text = now.format(&config.panel.clock_format).to_string();

        Self {
            panel_height: config.panel.height,
            at_top: config.panel.position == "top",
            show_clock: config.panel.show_clock,
            taskbar_mode: QString::from(config.panel.taskbar_mode.as_str()),
            clock_text: QString::from(clock_text.as_str()),
            clock_format: config.panel.clock_format.clone(),
            ..Default::default()
        }
    }

    fn update_clock(&mut self) {
        let now = chrono::Local::now();
        self.clock_text = QString::from(now.format(&self.clock_format).to_string().as_str());
        self.clock_changed();
    }

    fn launch_app(&self, cmd: String) {
        match std::process::Command::new(&cmd).spawn() {
            Ok(mut child) => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(e) => log::error!("Failed to launch {}: {}", cmd, e),
        }
    }
}

// ─── Taskbar Model ───────────────────────────────────────────────

#[allow(non_upper_case_globals)]
const Qt_UserRole: i32 = 0x0100;
const ROLE_WINDOW_ID: i32 = Qt_UserRole + 1;
const ROLE_TITLE: i32 = Qt_UserRole + 2;
const ROLE_APP_ID: i32 = Qt_UserRole + 3;
const ROLE_IS_ACTIVATED: i32 = Qt_UserRole + 4;
const ROLE_IS_MINIMIZED: i32 = Qt_UserRole + 5;
const ROLE_NERD_GLYPH: i32 = Qt_UserRole + 6;

struct TaskbarItem {
    id: u32,
    info: toplevel::ToplevelInfo,
}

#[derive(Default)]
struct TaskbarModelInner {
    items: Vec<TaskbarItem>,
    shared: Option<std::sync::Arc<std::sync::Mutex<toplevel::SharedState>>>,
    action_tx: Option<std::sync::mpsc::Sender<toplevel::ToplevelAction>>,
    last_generation: u64,
}

#[derive(QObject, Default)]
struct TaskbarModel {
    base: qt_base_class!(trait QAbstractListModel),
    inner: TaskbarModelInner,
    poll: qt_method!(fn(&mut self)),
    activate_window: qt_method!(fn(&self, window_id: u32)),
    close_window: qt_method!(fn(&self, window_id: u32)),
}

impl TaskbarModel {
    fn poll(&mut self) {
        let shared = match self.inner.shared.as_ref() {
            Some(s) => s,
            None => return,
        };

        let data = shared.lock().unwrap();
        if data.generation == self.inner.last_generation {
            return;
        }
        self.inner.last_generation = data.generation;

        (self as &dyn QAbstractListModel).begin_reset_model();
        self.inner.items.clear();
        for (&id, info) in &data.toplevels {
            if !info.title.is_empty() {
                self.inner.items.push(TaskbarItem {
                    id,
                    info: info.clone(),
                });
            }
        }
        (self as &dyn QAbstractListModel).end_reset_model();
    }

    fn activate_window(&self, window_id: u32) {
        if let Some(tx) = &self.inner.action_tx {
            let _ = tx.send(toplevel::ToplevelAction::Activate(window_id));
        }
    }

    fn close_window(&self, window_id: u32) {
        if let Some(tx) = &self.inner.action_tx {
            let _ = tx.send(toplevel::ToplevelAction::Close(window_id));
        }
    }
}

impl QAbstractListModel for TaskbarModel {
    fn row_count(&self) -> i32 {
        self.inner.items.len() as i32
    }

    fn data(&self, index: QModelIndex, role: i32) -> QVariant {
        let row = index.row() as usize;
        let item = match self.inner.items.get(row) {
            Some(it) => it,
            None => return QVariant::default(),
        };
        match role {
            ROLE_WINDOW_ID => QVariant::from(item.id as i32),
            ROLE_TITLE => QString::from(taskbar::truncate_title(&item.info.title, 25).as_str()).into(),
            ROLE_APP_ID => QString::from(item.info.app_id.as_str()).into(),
            ROLE_IS_ACTIVATED => QVariant::from(item.info.is_activated),
            ROLE_IS_MINIMIZED => QVariant::from(item.info.is_minimized),
            ROLE_NERD_GLYPH => QString::from(taskbar::nerd_glyph_for(&item.info.app_id).as_str()).into(),
            _ => QVariant::default(),
        }
    }

    fn role_names(&self) -> HashMap<i32, QByteArray> {
        let mut map = HashMap::new();
        map.insert(ROLE_WINDOW_ID, "windowId".into());
        map.insert(ROLE_TITLE, "title".into());
        map.insert(ROLE_APP_ID, "appId".into());
        map.insert(ROLE_IS_ACTIVATED, "isActivated".into());
        map.insert(ROLE_IS_MINIMIZED, "isMinimized".into());
        map.insert(ROLE_NERD_GLYPH, "nerdGlyph".into());
        map
    }
}

// ─── WiFi Model ──────────────────────────────────────────────────

const WIFI_ROLE_LABEL: i32 = Qt_UserRole + 1;
const WIFI_ROLE_SSID: i32 = Qt_UserRole + 2;

#[derive(QObject, Default)]
struct WifiModel {
    base: qt_base_class!(trait QAbstractListModel),
    networks: Vec<wifi::WifiNetwork>,
    refresh: qt_method!(fn(&mut self)),
    connect_to_network: qt_method!(fn(&self, index: i32)),
}

impl WifiModel {
    fn refresh(&mut self) {
        (self as &dyn QAbstractListModel).begin_reset_model();
        self.networks = wifi::scan_networks();
        (self as &dyn QAbstractListModel).end_reset_model();
    }

    fn connect_to_network(&self, index: i32) {
        if let Some(net) = self.networks.get(index as usize) {
            wifi::connect_network(&net.ssid);
        }
    }
}

impl QAbstractListModel for WifiModel {
    fn row_count(&self) -> i32 {
        self.networks.len() as i32
    }

    fn data(&self, index: QModelIndex, role: i32) -> QVariant {
        let row = index.row() as usize;
        let net = match self.networks.get(row) {
            Some(n) => n,
            None => return QVariant::default(),
        };
        match role {
            WIFI_ROLE_LABEL => {
                let label = wifi::format_network_label(net);
                QString::from(label.as_str()).into()
            }
            WIFI_ROLE_SSID => QString::from(net.ssid.as_str()).into(),
            _ => QVariant::default(),
        }
    }

    fn role_names(&self) -> HashMap<i32, QByteArray> {
        let mut map = HashMap::new();
        map.insert(WIFI_ROLE_LABEL, "label".into());
        map.insert(WIFI_ROLE_SSID, "ssid".into());
        map
    }
}

// ─── Main ────────────────────────────────────────────────────────

fn main() {
    env_logger::init();
    log::info!("Starting RDM Panel");

    let config = RdmConfig::load();

    // Enable layer-shell integration for Wayland
    std::env::set_var("QT_WAYLAND_SHELL_INTEGRATION", "layer-shell");

    // Start the Wayland toplevel tracker thread
    let (shared, action_tx) = toplevel::start_toplevel_tracker();

    // Create backends
    let panel = RefCell::new(PanelBackend::from_config(&config));

    let mut taskbar_model_inner = TaskbarModelInner::default();
    taskbar_model_inner.shared = Some(shared);
    taskbar_model_inner.action_tx = Some(action_tx);
    let taskbar_model = RefCell::new(TaskbarModel {
        inner: taskbar_model_inner,
        ..Default::default()
    });

    let tray_backend = RefCell::new(tray::TrayBackend::new());

    let wifi_model = RefCell::new(WifiModel::default());
    // Initial WiFi scan
    wifi_model.borrow_mut().refresh();

    let mut engine = QmlEngine::new();
    engine.set_object_property("_panel".into(), unsafe { QObjectPinned::new(&panel) });
    engine.set_object_property("_taskbarModel".into(), unsafe {
        QObjectPinned::new(&taskbar_model)
    });
    engine.set_object_property("_tray".into(), unsafe {
        QObjectPinned::new(&tray_backend)
    });
    engine.set_object_property("_wifiModel".into(), unsafe {
        QObjectPinned::new(&wifi_model)
    });
    engine.load_data(PANEL_QML.into());
    engine.exec();
}
