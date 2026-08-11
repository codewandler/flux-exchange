use std::path::Path;

#[test]
fn production_server_and_helpers_are_structurally_linux_only() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let build = std::fs::read_to_string(root.join("build.rs")).expect("server build guard");
    assert!(build.contains("CARGO_CFG_TARGET_OS"));
    assert!(build.contains("target_os != \"linux\""));
    assert!(build.contains("flux-exchange server supports Linux only"));

    let production = [
        "src/main.rs",
        "src/lib.rs",
        "src/local_helper_unix.rs",
        "src/local_management/mod.rs",
        "src/local_management/unix.rs",
        "src/local_management/service_account_handoff/unix_transfer.rs",
    ];
    for relative in production {
        let source = std::fs::read_to_string(root.join(relative)).expect("production source");
        for forbidden in [
            "cfg(windows)",
            "target_os = \"windows\"",
            "target_os = \"macos\"",
            "getpeereid",
            "local_helper_windows",
            "local_management::windows",
            "FXHA",
            "CONIN$",
        ] {
            assert!(
                !source.contains(forbidden),
                "{relative} retains non-Linux production selector {forbidden}"
            );
        }
    }

    let native_root =
        std::fs::read_to_string(root.join("src/native_root.rs")).expect("native root source");
    assert!(!native_root.contains("target_os = \"macos\""));
    assert!(!native_root.contains("getpeereid"));
    assert!(native_root.contains("#[cfg(target_os = \"linux\")]\nmod platform"));
}
