//! Windows-only build script: embed the app icon in `yutu-mux.exe`
//! (Explorer, taskbar, Alt-Tab). Every other target is a no-op: icons
//! there come from the Linux icon theme entries and the macOS `.icns`.
//!
//! Note the `TARGET` env check instead of `#[cfg(windows)]`: a build
//! script compiles for the *host*, so `cfg(windows)` is false during a
//! Linux MinGW cross build and the icon would silently go missing.

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if !target.contains("windows") {
        return;
    }
    // Fail the build rather than silently shipping an icon-less exe (the
    // bug this exists to prevent): MSVC needs rc.exe from the VS Build
    // Tools, MinGW cross builds need x86_64-w64-mingw32-windres.
    let mut res = winres::WindowsResource::new();
    res.set_icon("packaging/windows/yutu-mux.ico");
    if target.contains("windows-gnu") {
        // Cross builds: the resource compiler is the prefixed MinGW
        // windres, not plain `windres` (override with WINDRES when the
        // toolchain uses another prefix).
        let windres =
            std::env::var("WINDRES").unwrap_or_else(|_| "x86_64-w64-mingw32-windres".into());
        res.set_windres_path(&windres);
    }
    res.compile()
        .expect("embed yutu-mux.ico in yutu-mux.exe (see message above)");
    if target.contains("windows-gnu") {
        // Force the resource object onto the link line directly: winres
        // only exposes it through a static archive, and GNU ld
        // --gc-sections (on in release builds) never pulls unreferenced
        // archive members, which silently drops the icon again. A
        // directly passed object file is kept.
        let object = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("resource.o");
        println!("cargo:rustc-link-arg={}", object.display());
    }
}
