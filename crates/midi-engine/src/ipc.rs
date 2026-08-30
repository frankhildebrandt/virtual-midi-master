#[cfg(unix)]
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use virtual_midi_core::Config;

use crate::{EngineController, EngineSnapshot};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum IpcRequest {
    GetSnapshot,
    Subscribe { after_revision: u64 },
    GetConfig,
    ValidateConfig { config: Config },
    ApplyConfig { config: Config },
    Shutdown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcResponse {
    Snapshot { snapshot: EngineSnapshot },
    Config { config: Config },
    Valid,
    Applied { snapshot: EngineSnapshot },
    Ack,
    Error { message: String },
}

pub fn ipc_endpoint() -> PathBuf {
    #[cfg(unix)]
    {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        runtime.join(format!("virtual-midi-master-{}.sock", unsafe {
            libc_getuid()
        }))
    }
    #[cfg(windows)]
    {
        PathBuf::from(r"\\.\pipe\virtual-midi-master")
    }
}

#[cfg(unix)]
unsafe fn libc_getuid() -> u32 {
    unsafe extern "C" {
        fn getuid() -> u32;
    }
    unsafe { getuid() }
}

pub fn serve(controller: EngineController) -> Result<()> {
    #[cfg(unix)]
    return serve_unix(controller);
    #[cfg(windows)]
    return serve_windows(controller);
}

pub fn request(message: &IpcRequest) -> Result<IpcResponse> {
    #[cfg(unix)]
    return request_unix(message);
    #[cfg(windows)]
    return request_windows(message);
}

fn dispatch(controller: &EngineController, request: IpcRequest) -> IpcResponse {
    match request {
        IpcRequest::GetSnapshot => IpcResponse::Snapshot {
            snapshot: controller.snapshot(),
        },
        IpcRequest::Subscribe { after_revision } => {
            for _ in 0..20 {
                let snapshot = controller.snapshot();
                if snapshot.revision > after_revision || !snapshot.running {
                    return IpcResponse::Snapshot { snapshot };
                }
                thread::sleep(Duration::from_millis(50));
            }
            IpcResponse::Snapshot {
                snapshot: controller.snapshot(),
            }
        }
        IpcRequest::GetConfig => IpcResponse::Config {
            config: controller.snapshot().config,
        },
        IpcRequest::ValidateConfig { config } => match config.validate() {
            Ok(()) => IpcResponse::Valid,
            Err(error) => IpcResponse::Error {
                message: error.to_string(),
            },
        },
        IpcRequest::ApplyConfig { config } => match controller.apply_config(config) {
            Ok(()) => IpcResponse::Applied {
                snapshot: controller.snapshot(),
            },
            Err(error) => IpcResponse::Error {
                message: error.to_string(),
            },
        },
        IpcRequest::Shutdown => {
            controller.shutdown();
            IpcResponse::Ack
        }
    }
}

#[cfg(unix)]
fn serve_unix(controller: EngineController) -> Result<()> {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;

    let endpoint = ipc_endpoint();
    if endpoint.exists() {
        if std::os::unix::net::UnixStream::connect(&endpoint).is_ok() {
            return Err(anyhow!("Virtual MIDI Master engine is already running"));
        }
        fs::remove_file(&endpoint).context("remove stale IPC socket")?;
    }
    let listener = UnixListener::bind(&endpoint).context("bind IPC socket")?;
    fs::set_permissions(&endpoint, fs::Permissions::from_mode(0o600))
        .context("restrict IPC socket permissions")?;
    for stream in listener.incoming() {
        let controller = controller.clone();
        match stream {
            Ok(mut stream) => {
                thread::spawn(move || {
                    let read_stream = match stream.try_clone() {
                        Ok(stream) => stream,
                        Err(_) => return,
                    };
                    let mut line = String::new();
                    if BufReader::new(read_stream).read_line(&mut line).is_ok() {
                        let response = serde_json::from_str::<IpcRequest>(&line)
                            .map(|request| dispatch(&controller, request))
                            .unwrap_or_else(|error| IpcResponse::Error {
                                message: format!("invalid IPC request: {error}"),
                            });
                        if let Ok(json) = serde_json::to_vec(&response) {
                            let _ = stream.write_all(&json);
                            let _ = stream.write_all(b"\n");
                        }
                    }
                });
            }
            Err(error) => tracing::warn!(%error, "IPC connection failed"),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn request_unix(message: &IpcRequest) -> Result<IpcResponse> {
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(ipc_endpoint()).context("connect to MIDI engine")?;
    serde_json::to_writer(&mut stream, message).context("encode IPC request")?;
    stream.write_all(b"\n")?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    serde_json::from_str(&line).context("decode IPC response")
}

#[cfg(windows)]
fn serve_windows(controller: EngineController) -> Result<()> {
    use std::ffi::c_void;

    unsafe extern "C" {
        fn vmm_ipc_accept(buffer: *mut u8, capacity: u32, bytes_read: *mut u32) -> *mut c_void;
        fn vmm_ipc_reply(handle: *mut c_void, data: *const u8, length: u32) -> i32;
    }
    loop {
        let mut buffer = vec![0_u8; 1024 * 1024];
        let mut length = 0_u32;
        let handle =
            unsafe { vmm_ipc_accept(buffer.as_mut_ptr(), buffer.len() as u32, &mut length) };
        if handle.is_null() {
            return Err(anyhow!("accept Windows named-pipe connection failed"));
        }
        let controller = controller.clone();
        let handle_address = handle as usize;
        thread::spawn(move || {
            let response = serde_json::from_slice::<IpcRequest>(&buffer[..length as usize])
                .map(|request| dispatch(&controller, request))
                .unwrap_or_else(|error| IpcResponse::Error {
                    message: format!("invalid IPC request: {error}"),
                });
            let mut encoded = serde_json::to_vec(&response).unwrap_or_else(|error| {
                tracing::warn!(%error, "Windows named-pipe response encoding failed");
                br#"{"type":"error","message":"response encoding failed"}"#.to_vec()
            });
            encoded.push(b'\n');
            let handle = handle_address as *mut c_void;
            if unsafe { vmm_ipc_reply(handle, encoded.as_ptr(), encoded.len() as u32) } == 0 {
                tracing::warn!("Windows named-pipe reply failed");
            }
        });
    }
}

#[cfg(windows)]
fn request_windows(message: &IpcRequest) -> Result<IpcResponse> {
    unsafe extern "C" {
        fn vmm_ipc_request(
            request: *const u8,
            request_length: u32,
            response: *mut u8,
            response_capacity: u32,
            response_length: *mut u32,
        ) -> i32;
    }
    let mut encoded = serde_json::to_vec(message)?;
    encoded.push(b'\n');
    let mut response = vec![0_u8; 1024 * 1024];
    let mut response_length = 0_u32;
    if unsafe {
        vmm_ipc_request(
            encoded.as_ptr(),
            encoded.len() as u32,
            response.as_mut_ptr(),
            response.len() as u32,
            &mut response_length,
        )
    } == 0
    {
        return Err(anyhow!("connect to Windows engine named pipe failed"));
    }
    serde_json::from_slice(&response[..response_length as usize])
        .context("decode Windows IPC response")
}
