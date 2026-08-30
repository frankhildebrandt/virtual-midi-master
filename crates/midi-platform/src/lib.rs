mod memory;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use virtual_midi_core::{
    Config, EndpointDirection, EndpointId, EndpointKind, MidiEvent, VirtualDevice,
};

pub use memory::MemoryBackend;

pub fn configured_virtual_endpoints(devices: &[VirtualDevice], online: bool) -> Vec<EndpointInfo> {
    devices
        .iter()
        .filter(|device| device.enabled)
        .flat_map(|device| {
            [
                EndpointInfo {
                    id: EndpointId::new(format!("virtual:{}:in", device.id)),
                    name: format!("{} MIDI In", device.name),
                    direction: EndpointDirection::Source,
                    kind: EndpointKind::Virtual,
                    online,
                },
                EndpointInfo {
                    id: EndpointId::new(format!("virtual:{}:out", device.id)),
                    name: format!("{} MIDI Out", device.name),
                    direction: EndpointDirection::Target,
                    kind: EndpointKind::Virtual,
                    online,
                },
            ]
        })
        .collect()
}

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
    endpoints: Vec<EndpointInfo>,
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
            endpoints: Vec::new(),
        }
    }
}

#[cfg(target_os = "windows")]
impl MidiBackend for UnavailableBackend {
    fn reconfigure(&mut self, config: &Config) -> Result<Vec<EndpointInfo>, PlatformError> {
        self.endpoints = configured_virtual_endpoints(&config.virtual_devices, false);
        Ok(self.endpoints.clone())
    }

    fn endpoints(&self) -> Vec<EndpointInfo> {
        self.endpoints.clone()
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
