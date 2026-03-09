RDM Panel Plugins — Installation Guide
=======================================

WHAT IS A PLUGIN?
-----------------
RDM panel plugins are shared libraries (.so files) that add widgets to the panel
at runtime.  No recompiling rdm-panel is needed — just drop in a .so and edit
your rdm.toml.

Each plugin is a Rust (or C) cdylib crate compiled against rdm-panel-api.


WHERE DO PLUGINS LIVE?
----------------------
rdm-panel searches these directories in order (first match wins):

  1.  ~/.local/share/rdm/plugins/          ← user install (recommended)
  2.  /usr/local/lib/rdm/plugins/          ← system-wide (local prefix)
  3.  /usr/lib/rdm/plugins/                ← system-wide (distro package)
  4.  <rdm-panel binary dir>/rdm-plugins/  ← dev/portable use

Create the user directory if it doesn't exist:

    mkdir -p ~/.local/share/rdm/plugins


INSTALLING A PLUGIN
-------------------

Method 1 — Copy manually:

    cp librdm_panel_sysmon.so ~/.local/share/rdm/plugins/

Method 2 — Drag and drop:

    Drag any .so file onto  rdm-plugin-install.desktop
    (requires a file manager that supports drag-and-drop to .desktop launchers,
    e.g. Thunar, Nautilus, Nemo, Dolphin)

Method 3 — From source (build first):

    cd /path/to/RDM
    ./build-plugins.sh              # debug, or:
    ./build-plugins.sh --release    # optimised
    cp plugins/librdm_panel_sysmon.so ~/.local/share/rdm/plugins/


ENABLING A PLUGIN IN rdm.toml
------------------------------
Open  ~/.config/rdm/rdm.toml  and add a [[panel.plugins]] entry.
The  name  field must match what the plugin reports (rdm_plugin_info).

Example — sysmon:

    [[panel.plugins]]
    name     = "sysmon"
    position = "right"       # left | center | right

    [panel.plugins.config]
    interval_ms   = 1000     # refresh rate when popover is open
    process_limit = 8        # number of top-CPU processes to show
    show_network  = true     # show network RX/TX row
    button_label  = " Sys "  # text on the panel button

You can add as many [[panel.plugins]] blocks as you like; they appear in the
order listed.

After editing rdm.toml run  rdm-reload  (or re-log in) to pick up the changes.


REMOVING A PLUGIN
-----------------

1. Remove or comment out the [[panel.plugins]] block in rdm.toml.
2. Run  rdm-reload  so the panel restarts without loading it.
3. Optionally delete the .so file:

    rm ~/.local/share/rdm/plugins/librdm_panel_sysmon.so

The .so is NOT in use while rdm-panel is not running, so it is safe to delete
at any time.  While rdm-panel IS running the file is memory-mapped; on Linux
you can delete the inode and the running process keeps its copy until it exits.


WRITING YOUR OWN PLUGIN
-----------------------
See  crates/rdm-panel-api/src/lib.rs  for the full ABI documentation and the
rdm_export_plugin! macro.

Quick-start:

    # 1. Create a new crate
    cargo new --lib crates/rdm-panel-myplugin
    cd crates/rdm-panel-myplugin

    # 2. Edit Cargo.toml — add crate-type and deps:
    #    [lib]
    #    crate-type = ["cdylib"]
    #
    #    [dependencies]
    #    rdm-panel-api = { workspace = true }
    #    gtk4          = { workspace = true }

    # 3. In src/lib.rs implement your widget and call:
    #    rdm_export_plugin!(name: "myplugin", version: 1, new: MyPlugin::new, widget: MyPlugin::widget_ptr);

    # 4. Add "crates/rdm-panel-myplugin" to workspace members in Cargo.toml

    # 5. Build and test:
    #    ./build-plugins.sh
    #    cp plugins/librdm_panel_myplugin.so ~/.local/share/rdm/plugins/
    #    # add [[panel.plugins]] name = "myplugin" to rdm.toml
    #    rdm-reload

The four required exported C symbols are:
  rdm_plugin_info            → returns name + version
  rdm_plugin_new_instance    → creates and returns a GtkWidget*
  rdm_plugin_remove_instances → called when panel resets
  rdm_plugin_exit            → called on panel shutdown


BUILT-IN PLUGINS (shipped with RDM)
------------------------------------
  sysmon   crates/rdm-panel-sysmon   CPU / RAM / network popover


TROUBLESHOOTING
---------------
• Plugin not appearing    — check rdm.toml name matches rdm_plugin_info exactly
                            check the .so is in a scanned directory
                            check rdm-panel logs:  journalctl --user -u rdm-panel -f

• "failed to open" error  — the .so may be built against a different GTK version;
                            always rebuild plugins alongside rdm-panel

• ABI mismatch            — rebuild both rdm-panel and the plugin from the same
                            source tree; do not mix debug and release builds
