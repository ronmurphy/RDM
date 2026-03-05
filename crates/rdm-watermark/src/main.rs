use qmetaobject::prelude::*;
use std::cell::RefCell;

/// QML UI for the version watermark — a transparent bottom-layer surface in the
/// bottom-right corner of the desktop.  Layer-shell integration is provided by
/// the `org.kde.layershell` QML module (from `layer-shell-qt`).
const WATERMARK_QML: &str = r#"
import QtQuick 2.15
import QtQuick.Window 2.15
import org.kde.layershell 1.0 as LayerShell

Window {
    id: root
    visible: true
    width: 200
    height: 30
    color: "transparent"

    // Layer-shell: bottom layer, anchored to bottom-right, no exclusive zone
    LayerShell.Window.scope: "rdm-watermark"
    LayerShell.Window.layer: LayerShell.Window.LayerBottom
    LayerShell.Window.anchors: LayerShell.Window.AnchorBottom | LayerShell.Window.AnchorRight
    LayerShell.Window.exclusionZone: 0
    LayerShell.Window.bottomMargin: 8
    LayerShell.Window.rightMargin: 12

    Text {
        anchors.centerIn: parent
        text: _backend.versionText
        color: Qt.rgba(1, 1, 1, 0.25)
        font.family: "Inter"
        font.pixelSize: 11
    }
}
"#;

#[derive(QObject, Default)]
struct WatermarkBackend {
    base: qt_base_class!(trait QObject),
    version_text: qt_property!(QString; NOTIFY version_changed),
    version_changed: qt_signal!(),
}

fn main() {
    env_logger::init();
    log::info!("Starting RDM Watermark");

    let version = rdm_common::build_version_string();
    log::info!("Version: {}", version);

    // Enable layer-shell integration for Wayland
    std::env::set_var("QT_WAYLAND_SHELL_INTEGRATION", "layer-shell");

    let backend = RefCell::new(WatermarkBackend {
        version_text: QString::from(version.as_str()),
        ..Default::default()
    });

    let mut engine = QmlEngine::new();
    // SAFETY: `backend` lives on the stack and outlives `engine.exec()` which
    // blocks until the QML application exits, so the pinned reference is valid.
    engine.set_object_property("_backend".into(), unsafe { QObjectPinned::new(&backend) });
    engine.load_data(WATERMARK_QML.into());
    engine.exec();
}
