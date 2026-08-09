mod memory;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use virtual_midi_core::{Config, EndpointDirection, EndpointId, EndpointKind, MidiEvent};

pub use memory::MemoryBackend;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointInfo {
    pub id: EndpointId,
    pub name: String,
    pub direction: EndpointDirection,
    pub kind: EndpointKind,
    pub online: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformStatus {
    pub platform: String,
    pub runtime_available: bool,
    pub detail: String,
}

pub trait MidiBackend: Send {
    fn reconfigure(&mut self, config: &Config) -> Result<Vec<EndpointInfo>, PlatformError>;
    fn refresh(&mut self) -> Result<Option<Vec<EndpointInfo>>, PlatformError> {
        Ok(None)
    }
    fn take_queue_overflows(&self) -> u64 {
        0
    }
    fn endpoints(&self) -> Vec<EndpointInfo>;
    fn try_receive(&self) -> Option<MidiEvent>;
    fn send(&mut self, target: &EndpointId, data: &[u8]) -> Result<(), PlatformError>;
    fn status(&self) -> PlatformStatus;
}

#[derive(Debug, Error)]
pub enum PlatformError {
    #[error("MIDI platform initialization failed: {0}")]
    Initialization(String),
    #[error("MIDI endpoint operation failed: {0}")]
    Endpoint(String),
    #[error("MIDI runtime is unavailable: {0}")]
    RuntimeUnavailable(String),
}

pub fn create_system_backend(queue_capacity: usize) -> Result<Box<dyn MidiBackend>, PlatformError> {
    #[cfg(target_os = "macos")]
    {
        return Ok(Box::new(macos::MacOsBackend::new(queue_capacity)));
    }
    #[cfg(target_os = "windows")]
    {
        return Ok(match windows::WindowsBackend::new(queue_capacity) {
            Ok(backend) => Box::new(backend) as Box<dyn MidiBackend>,
            Err(error) => Box::new(UnavailableBackend::new(
                "windows-midi-services",
                error.to_string(),
            )),
        });
    }
    #[allow(unreachable_code)]
    Err(PlatformError::RuntimeUnavailable(
        "Virtual MIDI Master currently supports macOS and Windows 11".into(),
    ))
}

#[cfg(target_os = "windows")]
struct UnavailableBackend {
    status: PlatformStatus,
}

#[cfg(target_os = "windows")]
impl UnavailableBackend {
    fn new(platform: &str, detail: String) -> Self {
        Self {
            status: PlatformStatus {
                platform: platform.into(),
                runtime_available: false,
                detail,
            },
        }
    }
}

#[cfg(target_os = "windows")]
impl MidiBackend for UnavailableBackend {
    fn reconfigure(&mut self, _config: &Config) -> Result<Vec<EndpointInfo>, PlatformError> {
        Ok(Vec::new())
    }

    fn endpoints(&self) -> Vec<EndpointInfo> {
        Vec::new()
    }

    fn try_receive(&self) -> Option<MidiEvent> {
        None
    }

    fn send(&mut self, _target: &EndpointId, _data: &[u8]) -> Result<(), PlatformError> {
        Err(PlatformError::RuntimeUnavailable(
            self.status.detail.clone(),
        ))
    }

    fn status(&self) -> PlatformStatus {
        self.status.clone()
    }
}
