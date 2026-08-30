fn main() {
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rerun-if-env-changed=VMM_WINDOWS_MIDI_SDK_INCLUDE");
        println!("cargo:rerun-if-changed=windows/bridge.cpp");
        println!("cargo:rerun-if-changed=windows/bridge.h");
        let includes = std::env::var_os("VMM_WINDOWS_MIDI_SDK_INCLUDE").unwrap_or_else(|| {
            panic!(
                "VMM_WINDOWS_MIDI_SDK_INCLUDE is not set. Build through `make` or scripts/with-windows-midi-sdk.ps1 so the Windows MIDI Services SDK headers are generated."
            )
        });
        let mut build = cc::Build::new();
        build.cpp(true).std("c++20").file("windows/bridge.cpp");
        for include in std::env::split_paths(&includes) {
            build.include(include);
        }
        build.compile("virtual_midi_windows_bridge");
        println!("cargo:rustc-link-lib=runtimeobject");
        println!("cargo:rustc-link-lib=advapi32");
        println!("cargo:rustc-link-lib=ole32");
    }
}
