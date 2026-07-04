//! Windows-only: embed the app icon and version metadata into the `.exe`
//! as a PE resource, so Explorer, the taskbar, and Alt-Tab show it. A no-op
//! on every other platform — macOS icons come from the `.app` bundle and
//! Linux from hicolor + `.desktop` files at packaging time.

fn main() {
    // The dependency is target-keyed and the compiled resource is
    // host-tooled (rc.exe/windres), so gate on both: the `#[cfg]` is the
    // build script's *host*, the env var the *target*.
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let icon = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/icons/preso.ico");
        println!("cargo:rerun-if-changed={icon}");
        let mut res = winresource::WindowsResource::new();
        res.set_icon(icon);
        res.set("ProductName", "preso");
        res.set("FileDescription", "Native markdown presentations");
        if let Err(e) = res.compile() {
            // Branding isn't worth failing the build over; surface it.
            println!("cargo:warning=could not embed the Windows icon: {e}");
        }
    }
}
