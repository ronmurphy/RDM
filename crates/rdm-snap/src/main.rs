use rdm_common::config::RdmConfig;

fn main() {
    env_logger::init();
    log::info!("Starting RDM Snap daemon");

    let config = RdmConfig::load();
    log::info!(
        "Snap config: edge_threshold={}, show_preview={}",
        config.snap.edge_threshold,
        config.snap.show_preview
    );

    // The snap daemon enhances labwc's built-in window snapping.
    // labwc already supports basic edge snapping via its rc.xml config.
    //
    // This daemon will:
    // 1. Monitor pointer position near screen edges via Wayland protocols
    // 2. Show a translucent preview overlay (layer-shell surface) of the snap zone
    // 3. Provide corner-snapping (quarter tiling) and thirds support
    //
    // For now, we rely on labwc's built-in snapping and configure it properly.
    // The visual overlay will be added as we iterate.

    log::info!("Snap daemon running — relying on labwc built-in snapping for now");
    log::info!("Configure labwc snap zones in ~/.config/labwc/rc.xml");

    // Keep running to be ready for future functionality
    // In production this will run an event loop monitoring pointer/toplevel state
    std::thread::park();
}
