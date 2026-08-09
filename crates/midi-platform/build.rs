fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut build = cc::Build::new();
        build.cpp(true).std("c++20").file("windows/bridge.cpp");
        if let Some(includes) = std::env::var_os("VMM_WINDOWS_MIDI_SDK_INCLUDE") {
            for include in std::env::split_paths(&includes) {
                build.include(include);
            }
        }
        build.compile("virtual_midi_windows_bridge");
        println!("cargo:rustc-link-lib=runtimeobject");
        println!("cargo:rustc-link-lib=advapi32");
    }
}
