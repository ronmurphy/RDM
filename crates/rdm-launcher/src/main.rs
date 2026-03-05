use qmetaobject::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;

mod desktop_apps;

// ─── QML UI ──────────────────────────────────────────────────────

/// QML overlay launcher with layer-shell.  Provides a search box + scrollable
/// list of `.desktop` applications.  Escape key closes the launcher.
const LAUNCHER_QML: &str = r#"
import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import QtQuick.Window 2.15
import org.kde.layershell 1.0 as LayerShell

Window {
    id: root
    visible: true
    width: _backend.launcherWidth
    height: _backend.launcherHeight
    color: "transparent"

    LayerShell.Window.scope: "rdm-launcher"
    LayerShell.Window.layer: LayerShell.Window.LayerOverlay
    LayerShell.Window.keyboardInteractivity: LayerShell.Window.KeyboardInteractivityExclusive

    Rectangle {
        anchors.fill: parent
        radius: 12
        color: "#1a1b26"

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 16
            spacing: 8

            Text {
                text: "Launch Application"
                color: "#7aa2f7"
                font.pixelSize: 18
                font.bold: true
            }

            TextField {
                id: searchField
                Layout.fillWidth: true
                placeholderText: "Type to search..."
                color: "#c0caf5"
                placeholderTextColor: "#565f89"
                font.pixelSize: 14
                background: Rectangle {
                    color: "#24283b"
                    border.color: "#3b4261"
                    border.width: 1
                    radius: 8
                }
                padding: 8
                onTextChanged: _appModel.filterText = text
                Keys.onEscapePressed: Qt.quit()
                Component.onCompleted: forceActiveFocus()
                Keys.onReturnPressed: {
                    if (appList.count > 0) {
                        _appModel.launch(0)
                        Qt.quit()
                    }
                }
            }

            ListView {
                id: appList
                Layout.fillWidth: true
                Layout.fillHeight: true
                model: _appModel
                clip: true
                currentIndex: 0
                highlight: Rectangle { color: "#292e42"; radius: 6 }
                highlightFollowsCurrentItem: true

                delegate: Item {
                    width: appList.width
                    height: 36

                    MouseArea {
                        anchors.fill: parent
                        onClicked: {
                            _appModel.launch(index)
                            Qt.quit()
                        }
                    }

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 8
                        anchors.rightMargin: 8
                        spacing: 8

                        Text {
                            text: model.name
                            color: "#c0caf5"
                            font.pixelSize: 13
                        }
                        Text {
                            text: model.comment
                            color: "#565f89"
                            font.pixelSize: 11
                            Layout.fillWidth: true
                            horizontalAlignment: Text.AlignRight
                            elide: Text.ElideRight
                        }
                    }
                }

                Keys.onUpPressed: decrementCurrentIndex()
                Keys.onDownPressed: incrementCurrentIndex()
            }
        }
    }

    Shortcut {
        sequence: "Escape"
        onActivated: Qt.quit()
    }
}
"#;

// ─── App List Model ──────────────────────────────────────────────

const ROLE_NAME: i32 = Qt_UserRole + 1;
const ROLE_COMMENT: i32 = Qt_UserRole + 2;
const ROLE_EXEC: i32 = Qt_UserRole + 3;

#[allow(non_upper_case_globals)]
const Qt_UserRole: i32 = 0x0100;

#[derive(QObject, Default)]
struct AppListModel {
    base: qt_base_class!(trait QAbstractListModel),
    all_entries: Vec<desktop_apps::AppEntry>,
    filtered: Vec<usize>,
    filter_text: qt_property!(QString; WRITE set_filter_text),
    launch: qt_method!(fn(&self, index: i32)),
}

impl AppListModel {
    fn set_filter_text(&mut self, text: QString) {
        let query = text.to_string().to_lowercase();
        (self as &dyn QAbstractListModel).begin_reset_model();
        if query.is_empty() {
            self.filtered = (0..self.all_entries.len()).collect();
        } else {
            self.filtered = self
                .all_entries
                .iter()
                .enumerate()
                .filter(|(_, e)| {
                    e.name.to_lowercase().contains(&query)
                        || e.comment
                            .as_ref()
                            .map(|c| c.to_lowercase().contains(&query))
                            .unwrap_or(false)
                })
                .map(|(i, _)| i)
                .collect();
        }
        // Limit to 50 results
        self.filtered.truncate(50);
        (self as &dyn QAbstractListModel).end_reset_model();
    }

    fn launch(&self, index: i32) {
        if let Some(&entry_idx) = self.filtered.get(index as usize) {
            if let Some(entry) = self.all_entries.get(entry_idx) {
                launch_app(&entry.exec);
            }
        }
    }
}

impl QAbstractListModel for AppListModel {
    fn row_count(&self) -> i32 {
        self.filtered.len() as i32
    }

    fn data(&self, index: QModelIndex, role: i32) -> QVariant {
        let row = index.row() as usize;
        let entry_idx = match self.filtered.get(row) {
            Some(&i) => i,
            None => return QVariant::default(),
        };
        let entry = match self.all_entries.get(entry_idx) {
            Some(e) => e,
            None => return QVariant::default(),
        };
        match role {
            ROLE_NAME => QString::from(entry.name.as_str()).into(),
            ROLE_COMMENT => QString::from(entry.comment.as_deref().unwrap_or("")).into(),
            ROLE_EXEC => QString::from(entry.exec.as_str()).into(),
            _ => QVariant::default(),
        }
    }

    fn role_names(&self) -> HashMap<i32, QByteArray> {
        let mut map = HashMap::new();
        map.insert(ROLE_NAME, "name".into());
        map.insert(ROLE_COMMENT, "comment".into());
        map.insert(ROLE_EXEC, "exec".into());
        map
    }
}

// ─── Launcher Backend ────────────────────────────────────────────

#[derive(QObject, Default)]
struct LauncherBackend {
    base: qt_base_class!(trait QObject),
    launcher_width: qt_property!(i32; NOTIFY config_changed),
    launcher_height: qt_property!(i32; NOTIFY config_changed),
    config_changed: qt_signal!(),
}

// ─── App Launching ───────────────────────────────────────────────

fn launch_app(exec: &str) {
    // Strip field codes like %f, %u, %F, %U from Exec line
    let cmd: String = exec
        .split_whitespace()
        .filter(|s| !s.starts_with('%'))
        .collect::<Vec<_>>()
        .join(" ");

    log::info!("Launching: {}", cmd);

    match std::process::Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => log::error!("Failed to launch '{}': {}", cmd, e),
    }
}

// ─── Main ────────────────────────────────────────────────────────

fn main() {
    env_logger::init();
    log::info!("Starting RDM Launcher");

    let config = rdm_common::config::RdmConfig::load();

    // Enable layer-shell integration for Wayland
    std::env::set_var("QT_WAYLAND_SHELL_INTEGRATION", "layer-shell");

    // Load desktop entries
    let entries = desktop_apps::load_desktop_entries();
    log::info!("Loaded {} desktop entries", entries.len());

    let filtered: Vec<usize> = (0..entries.len()).collect();
    let model = RefCell::new(AppListModel {
        all_entries: entries,
        filtered,
        ..Default::default()
    });

    let backend = RefCell::new(LauncherBackend {
        launcher_width: config.launcher.width,
        launcher_height: config.launcher.height,
        ..Default::default()
    });

    let mut engine = QmlEngine::new();
    engine.set_object_property("_appModel".into(), unsafe { QObjectPinned::new(&model) });
    engine.set_object_property("_backend".into(), unsafe { QObjectPinned::new(&backend) });
    engine.load_data(LAUNCHER_QML.into());
    engine.exec();
}
