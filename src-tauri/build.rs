fn main() {
    // Declaring our own commands here is what makes them ACL-checkable at
    // all: `tauri_build::build()` (the bare call this used to be) never told
    // Tauri which commands the app registers, so `AppManifest::commands` was
    // empty and no `allow-<command>` permissions were ever generated for them
    // — every invoke() of get_server_token (and everything else below) was
    // hard-denied by the ACL before it could run, regardless of what the
    // capability file granted. This was the actual root cause of the
    // "unauthorized" bug: the auth token could never load because the very
    // command that fetches it was never a grantable permission in the first
    // place. See capabilities/main.json, which references the resulting
    // `allow-<command>` permissions.
    //
    // This list must match src/lib.rs's own `invoke_handler(generate_handler![...])`
    // exactly — 5 real commands (list_monitors, ndi_update_media/timer,
    // syphon_update_media/timer) had been registered there and called from
    // the frontend for a while without ever being added here, so every call
    // to them was silently hard-denied by the ACL regardless of the matching
    // `allow-*` entry already sitting in capabilities/main.json. list_monitors
    // backs the entire display/output picker; the update_media/update_timer
    // pair is what makes NDI/Syphon outputs show media and the timer at all.
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new().commands(&[
                "get_server_port",
                "get_server_token",
                "get_server_config",
                "list_monitors",
                "list_system_fonts",
                "signal_main_ready",
                "install_update",
                "ndi_available",
                "ndi_start",
                "ndi_stop",
                "ndi_update",
                "ndi_update_media",
                "ndi_update_timer",
                "output_capture",
                "output_capture_supported",
                "output_video",
                "decklink_list",
                "decklink_start",
                "decklink_stop",
                "syphon_available",
                "syphon_start",
                "syphon_stop",
                "syphon_update",
                "syphon_update_media",
                "syphon_update_timer",
                "stream_start",
                "stream_stop",
                "stream_status",
                "stream_program",
                "stream_mix",
                "stream_cameras",
                "record_start",
                "record_stop",
                "record_status",
                "record_reveal",
            ]),
        ),
    )
    .expect("tauri_build failed");

    // ── macOS-only: link & bundle Syphon.framework ───────────────────────
    // Syphon is the standard macOS shared-texture protocol used by ProPresenter,
    // OBS (via plugin), Resolume, MadMapper, etc. We bundle the framework into
    // the .app's Contents/Frameworks dir (Tauri does that via tauri.conf.json's
    // bundle.macOS.frameworks) and tell rustc to link against it at build time.
    //
    // The @executable_path-relative rpath ensures dyld finds the framework at
    // runtime — both inside the bundled .app (where the binary lives at
    // Contents/MacOS/kairo and the framework at Contents/Frameworks/Syphon.framework)
    // and during cargo-run dev builds (where we point at the in-tree Frameworks/).
    //
    // Deliberately checked via CARGO_CFG_TARGET_OS (the env var Cargo sets to
    // the actual TARGET being built), not #[cfg(target_os = "macos")] — a
    // build.rs's own #[cfg] attributes reflect the HOST it's compiled and run
    // on, not the target, since build scripts always execute on the host.
    // On a real Mac building for itself the two happen to coincide, which
    // hid this; it surfaces the moment anyone cross-compiles the Windows
    // build from a macOS/Linux CI runner (confirmed via `cargo xwin check
    // --target x86_64-pc-windows-msvc`, which failed with "library kind
    // `framework` is only supported on Apple targets" under the old check).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        println!("cargo:rustc-link-search=framework={}/Frameworks", manifest);
        println!("cargo:rustc-link-lib=framework=Syphon");
        // Production rpath: points at the framework copied into the .app bundle.
        println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
        // Dev rpath: lets `cargo run` from src-tauri/ find the in-tree copy.
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}/Frameworks", manifest);
        println!("cargo:rerun-if-changed=Frameworks/Syphon.framework/Syphon");
    }
    build_decklink();
}

/// DeckLink output (native/decklink.cpp over vendor/decklink's API headers).
/// Sets cfg `kairo_decklink` when it builds; without it (no compiler step,
/// MIDL missing on Windows) Kairo builds without DeckLink and says so in
/// Settings rather than failing the whole build.
fn build_decklink() {
    use std::path::PathBuf;
    println!("cargo:rustc-check-cfg=cfg(kairo_decklink)");
    println!("cargo:rerun-if-changed=native/decklink.cpp");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let vendor = manifest.join("vendor/decklink");
    let mut b = cc::Build::new();
    b.cpp(true).file(manifest.join("native/decklink.cpp")).warnings(false);
    match os.as_str() {
        "macos" => {
            b.include(vendor.join("mac")).file(vendor.join("mac/DeckLinkAPIDispatch.cpp")).flag_if_supported("-std=c++17");
            println!("cargo:rustc-link-lib=framework=CoreFoundation");
        }
        "linux" => {
            b.include(vendor.join("linux")).file(vendor.join("linux/DeckLinkAPIDispatch.cpp")).flag_if_supported("-std=c++17");
            println!("cargo:rustc-link-lib=dl");
            println!("cargo:rustc-link-lib=pthread");
        }
        "windows" => {
            // The Windows API ships as IDL: MIDL (Windows SDK) makes the header
            // and the interface IDs, run with the compiler's own environment.
            let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
            let target = std::env::var("TARGET").unwrap_or_default();
            let win = vendor.join("win");
            let ok = cc::windows_registry::find_tool(&target, "cl.exe").is_some_and(|cl| {
                let mut cmd = std::process::Command::new("midl.exe");
                for (k, v) in cl.env() {
                    cmd.env(k, v);
                }
                cmd.current_dir(&out)
                    .args(["/nologo", "/env", "x64", "/h", "DeckLinkAPI_h.h", "/iid", "DeckLinkAPI_i.c", "/I"])
                    .arg(&win)
                    .arg(win.join("DeckLinkAPI.idl"))
                    .status()
                    .is_ok_and(|s| s.success())
            });
            if !ok {
                println!("cargo:warning=DeckLink output left out: MIDL (Windows SDK) not found to process the DeckLink IDL.");
                return;
            }
            b.include(&out).file(out.join("DeckLinkAPI_i.c"));
            println!("cargo:rustc-link-lib=ole32");
            println!("cargo:rustc-link-lib=oleaut32");
        }
        _ => return,
    }
    b.compile("kairo_decklink");
    println!("cargo:rustc-cfg=kairo_decklink");
}
