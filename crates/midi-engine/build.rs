fn main() {
    #[cfg(target_os = "windows")]
    {
        cc::Build::new()
            .cpp(true)
            .std("c++20")
            .file("windows/ipc_bridge.cpp")
            .compile("virtual_midi_engine_ipc_bridge");
        println!("cargo:rustc-link-lib=advapi32");
    }
}
