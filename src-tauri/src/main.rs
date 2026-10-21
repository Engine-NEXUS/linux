fn main() {
    // Linux: pick the GTK backend BEFORE Tauri initialises GTK. Under native
    // Wayland the overlay cannot be kept on top, hidden from the taskbar or
    // positioned, so we run on XWayland when one is available. The decision
    // (and why) is logged — see `gdk_backend_linux`.
    #[cfg(target_os = "linux")]
    nexus_lib::gdk_backend_linux::apply();

    nexus_lib::run()
}
