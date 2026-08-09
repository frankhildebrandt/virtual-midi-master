use anyhow::{Context, Result};

pub fn set_autostart(enabled: bool) -> Result<()> {
    if std::env::var_os("VMM_DISABLE_AUTOSTART").is_some() {
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    return set_macos_autostart(enabled);
    #[cfg(target_os = "windows")]
    return set_windows_autostart(enabled);
    #[allow(unreachable_code)]
    Ok(())
}

#[cfg(target_os = "macos")]
fn set_macos_autostart(enabled: bool) -> Result<()> {
    use std::fs;
    use std::io::Write;

    let home = dirs::home_dir().context("home directory is unavailable")?;
    let directory = home.join("Library/LaunchAgents");
    let path = directory.join("com.virtualmidimaster.engine.plist");
    if !enabled {
        if path.exists() {
            fs::remove_file(path).context("remove LaunchAgent")?;
        }
        return Ok(());
    }
    fs::create_dir_all(&directory).context("create LaunchAgents directory")?;
    let executable = std::env::current_exe().context("resolve engine executable")?;
    let escaped = executable
        .to_string_lossy()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let plist = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>com.virtualmidimaster.engine</string><key>ProgramArguments</key><array><string>{escaped}</string></array><key>RunAtLoad</key><true/><key>KeepAlive</key><true/></dict></plist>\n"
    );
    let temporary = path.with_extension("plist.tmp");
    fs::File::create(&temporary)?.write_all(plist.as_bytes())?;
    fs::rename(temporary, path).context("install LaunchAgent")
}

#[cfg(target_os = "windows")]
fn set_windows_autostart(enabled: bool) -> Result<()> {
    use windows::Win32::System::Registry::{
        HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW, RegOpenKeyExW,
        RegSetValueExW,
    };
    use windows::core::PCWSTR;

    let key_path: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Run\0"
        .encode_utf16()
        .collect();
    let value_name: Vec<u16> = "VirtualMidiMasterEngine\0".encode_utf16().collect();
    unsafe {
        let mut key = Default::default();
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key_path.as_ptr()),
            0,
            KEY_SET_VALUE,
            &mut key,
        )
        .ok()
        .context("open Windows Run key")?;
        if enabled {
            let value = format!("\"{}\"\0", std::env::current_exe()?.display());
            let wide: Vec<u16> = value.encode_utf16().collect();
            RegSetValueExW(
                key,
                PCWSTR(value_name.as_ptr()),
                0,
                REG_SZ,
                Some(std::slice::from_raw_parts(
                    wide.as_ptr().cast(),
                    wide.len() * 2,
                )),
            )
            .ok()
            .context("set Windows autostart")?;
        } else {
            let _ = RegDeleteValueW(key, PCWSTR(value_name.as_ptr()));
        }
        let _ = RegCloseKey(key);
    }
    Ok(())
}
