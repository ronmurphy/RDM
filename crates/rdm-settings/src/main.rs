use qmetaobject::prelude::*;
use rdm_common::config::RdmConfig;
use std::cell::RefCell;

// ─── QML UI ──────────────────────────────────────────────────────

/// Settings window — a regular (non-layer-shell) Qt window with a sidebar +
/// stack layout for panel and wallpaper configuration pages.
const SETTINGS_QML: &str = r#"
import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import QtQuick.Window 2.15
import QtQuick.Dialogs

Window {
    id: root
    visible: true
    width: 520
    height: 480
    title: "RDM Settings"
    color: "#1a1b26"

    RowLayout {
        anchors.fill: parent
        spacing: 0

        // Sidebar
        ListView {
            id: sidebar
            Layout.preferredWidth: 140
            Layout.fillHeight: true
            model: ["Panel", "Wallpaper"]
            currentIndex: 0
            delegate: ItemDelegate {
                width: sidebar.width
                height: 40
                highlighted: ListView.isCurrentItem
                onClicked: sidebar.currentIndex = index
                contentItem: Text {
                    text: modelData
                    color: highlighted ? "#ffffff" : "#c0caf5"
                    font.pixelSize: 14
                    verticalAlignment: Text.AlignVCenter
                    leftPadding: 12
                }
                background: Rectangle {
                    color: highlighted ? "#3d59a1" : "transparent"
                }
            }
            Rectangle {
                anchors.fill: parent
                z: -1
                color: "#16161e"
            }
        }

        // Separator
        Rectangle { width: 1; Layout.fillHeight: true; color: "#3b4261" }

        // Stack
        StackLayout {
            currentIndex: sidebar.currentIndex
            Layout.fillWidth: true
            Layout.fillHeight: true

            // ─── Panel Page ─────────────────────────
            ScrollView {
                ColumnLayout {
                    width: parent.width
                    spacing: 16

                    Item { Layout.preferredHeight: 20 }

                    Text {
                        text: "Panel"
                        color: "#7aa2f7"
                        font.pixelSize: 18
                        font.bold: true
                        Layout.leftMargin: 20
                    }

                    // Taskbar Mode
                    RowLayout {
                        Layout.leftMargin: 20; Layout.rightMargin: 20; spacing: 12
                        Text { text: "Taskbar Mode"; color: "#c0caf5"; Layout.preferredWidth: 140 }
                        ComboBox {
                            id: taskbarModeCombo
                            model: ["icons", "text", "nerd"]
                            currentIndex: _settings.taskbarModeIndex
                            onCurrentIndexChanged: _settings.taskbarModeIndex = currentIndex
                            Layout.preferredWidth: 180
                        }
                    }

                    // Panel Position
                    RowLayout {
                        Layout.leftMargin: 20; Layout.rightMargin: 20; spacing: 12
                        Text { text: "Panel Position"; color: "#c0caf5"; Layout.preferredWidth: 140 }
                        ComboBox {
                            id: posCombo
                            model: ["top", "bottom"]
                            currentIndex: _settings.positionIndex
                            onCurrentIndexChanged: _settings.positionIndex = currentIndex
                            Layout.preferredWidth: 180
                        }
                    }

                    // Panel Height
                    RowLayout {
                        Layout.leftMargin: 20; Layout.rightMargin: 20; spacing: 12
                        Text { text: "Panel Height"; color: "#c0caf5"; Layout.preferredWidth: 140 }
                        SpinBox {
                            id: heightSpin
                            from: 24; to: 64; value: _settings.panelHeight
                            onValueChanged: _settings.panelHeight = value
                        }
                    }

                    // Show Clock
                    RowLayout {
                        Layout.leftMargin: 20; Layout.rightMargin: 20; spacing: 12
                        Text { text: "Show Clock"; color: "#c0caf5"; Layout.preferredWidth: 140 }
                        Switch {
                            id: clockSwitch
                            checked: _settings.showClock
                            onCheckedChanged: _settings.showClock = checked
                        }
                    }

                    // Clock Format
                    RowLayout {
                        Layout.leftMargin: 20; Layout.rightMargin: 20; spacing: 12
                        Text { text: "Clock Format"; color: "#c0caf5"; Layout.preferredWidth: 140 }
                        TextField {
                            id: clockFmtField
                            text: _settings.clockFormat
                            onTextChanged: _settings.clockFormat = text
                            color: "#c0caf5"
                            Layout.fillWidth: true
                            background: Rectangle {
                                color: "#292e42"
                                border.color: "#3b4261"
                                border.width: 1
                                radius: 6
                            }
                        }
                    }

                    Item { Layout.fillHeight: true }
                }
            }

            // ─── Wallpaper Page ─────────────────────
            ScrollView {
                ColumnLayout {
                    width: parent.width
                    spacing: 16

                    Item { Layout.preferredHeight: 20 }

                    Text {
                        text: "Wallpaper"
                        color: "#7aa2f7"
                        font.pixelSize: 18
                        font.bold: true
                        Layout.leftMargin: 20
                    }

                    // Image path + Browse + Clear
                    RowLayout {
                        Layout.leftMargin: 20; Layout.rightMargin: 20; spacing: 8
                        Text { text: "Image"; color: "#c0caf5"; Layout.preferredWidth: 140 }
                        Text {
                            id: wpPathLabel
                            text: _settings.wallpaperPath || "(none — solid color)"
                            color: "#a9b1d6"
                            font.pixelSize: 12
                            elide: Text.ElideMiddle
                            Layout.fillWidth: true
                        }
                        Button {
                            text: "Browse…"
                            onClicked: fileDialog.open()
                        }
                        Button {
                            text: "Clear"
                            onClicked: _settings.wallpaperPath = ""
                        }
                    }

                    // Wallpaper mode
                    RowLayout {
                        Layout.leftMargin: 20; Layout.rightMargin: 20; spacing: 12
                        Text { text: "Mode"; color: "#c0caf5"; Layout.preferredWidth: 140 }
                        ComboBox {
                            model: ["fill", "center", "stretch", "fit", "tile"]
                            currentIndex: _settings.wallpaperModeIndex
                            onCurrentIndexChanged: _settings.wallpaperModeIndex = currentIndex
                            Layout.preferredWidth: 180
                        }
                    }

                    // Background color
                    RowLayout {
                        Layout.leftMargin: 20; Layout.rightMargin: 20; spacing: 12
                        Text { text: "Background Color"; color: "#c0caf5"; Layout.preferredWidth: 140 }
                        TextField {
                            text: _settings.wallpaperColor
                            onTextChanged: _settings.wallpaperColor = text
                            maximumLength: 10
                            color: "#c0caf5"
                            Layout.preferredWidth: 120
                            background: Rectangle {
                                color: "#292e42"
                                border.color: "#3b4261"
                                border.width: 1
                                radius: 6
                            }
                        }
                    }

                    Text {
                        text: "Changes apply after clicking Apply. Panel will hot-reload."
                        color: "#565f89"
                        font.pixelSize: 11
                        font.italic: true
                        Layout.leftMargin: 20
                        Layout.topMargin: 12
                    }

                    Item { Layout.fillHeight: true }
                }
            }
        }
    }

    // ─── Bottom bar ─────────────────────────────
    footer: ToolBar {
        background: Rectangle { color: "#1a1b26" }
        RowLayout {
            anchors.right: parent.right
            anchors.rightMargin: 12
            anchors.verticalCenter: parent.verticalCenter
            spacing: 8
            Button {
                text: "Cancel"
                onClicked: Qt.quit()
            }
            Button {
                text: "Apply"
                onClicked: {
                    _settings.apply()
                    Qt.quit()
                }
            }
        }
    }

    FileDialog {
        id: fileDialog
        title: "Choose Wallpaper"
        nameFilters: ["Images (*.png *.jpg *.jpeg *.webp *.bmp)"]
        onAccepted: {
            var path = selectedFile.toString()
            // Strip file:// prefix
            if (path.startsWith("file://")) path = path.substring(7)
            _settings.wallpaperPath = path
        }
    }
}
"#;

// ─── Settings Backend ────────────────────────────────────────────

#[derive(QObject, Default)]
struct SettingsBackend {
    base: qt_base_class!(trait QObject),

    // Panel settings
    taskbar_mode_index: qt_property!(i32; NOTIFY settings_changed),
    position_index: qt_property!(i32; NOTIFY settings_changed),
    panel_height: qt_property!(i32; NOTIFY settings_changed),
    show_clock: qt_property!(bool; NOTIFY settings_changed),
    clock_format: qt_property!(QString; NOTIFY settings_changed),

    // Wallpaper settings
    wallpaper_path: qt_property!(QString; NOTIFY settings_changed),
    wallpaper_mode_index: qt_property!(i32; NOTIFY settings_changed),
    wallpaper_color: qt_property!(QString; NOTIFY settings_changed),

    settings_changed: qt_signal!(),
    apply: qt_method!(fn(&self)),
}

impl SettingsBackend {
    fn from_config(config: &RdmConfig) -> Self {
        let taskbar_mode_index = match config.panel.taskbar_mode.as_str() {
            "icons" => 0,
            "text" => 1,
            "nerd" => 2,
            _ => 0,
        };
        let position_index = if config.panel.position == "bottom" { 1 } else { 0 };
        let wallpaper_mode_index = match config.wallpaper.mode.as_str() {
            "fill" => 0,
            "center" => 1,
            "stretch" => 2,
            "fit" => 3,
            "tile" => 4,
            _ => 0,
        };

        Self {
            taskbar_mode_index,
            position_index,
            panel_height: config.panel.height,
            show_clock: config.panel.show_clock,
            clock_format: QString::from(config.panel.clock_format.as_str()),
            wallpaper_path: QString::from(config.wallpaper.path.as_str()),
            wallpaper_mode_index,
            wallpaper_color: QString::from(config.wallpaper.color.as_str()),
            ..Default::default()
        }
    }

    fn apply(&self) {
        let mut config = RdmConfig::load();

        config.panel.taskbar_mode = match self.taskbar_mode_index {
            1 => "text",
            2 => "nerd",
            _ => "icons",
        }
        .to_string();
        config.panel.position = if self.position_index == 1 {
            "bottom"
        } else {
            "top"
        }
        .to_string();
        config.panel.height = self.panel_height;
        config.panel.show_clock = self.show_clock;
        config.panel.clock_format = self.clock_format.to_string();

        config.wallpaper.path = self.wallpaper_path.to_string();
        config.wallpaper.mode = match self.wallpaper_mode_index {
            0 => "fill",
            1 => "center",
            2 => "stretch",
            3 => "fit",
            4 => "tile",
            _ => "fill",
        }
        .to_string();
        config.wallpaper.color = self.wallpaper_color.to_string();

        match config.save() {
            Ok(()) => {
                log::info!("Config saved, applying changes...");
                let _ = std::process::Command::new("rdm-reload").status();
            }
            Err(e) => {
                log::error!("Failed to save config: {}", e);
            }
        }
    }
}

// ─── Main ────────────────────────────────────────────────────────

fn main() {
    env_logger::init();

    let config = RdmConfig::load();

    let backend = RefCell::new(SettingsBackend::from_config(&config));

    let mut engine = QmlEngine::new();
    // SAFETY: `backend` lives on the stack and outlives `engine.exec()` which
    // blocks until the QML application exits, so the pinned reference is valid.
    engine.set_object_property("_settings".into(), unsafe {
        QObjectPinned::new(&backend)
    });
    engine.load_data(SETTINGS_QML.into());
    engine.exec();
}
