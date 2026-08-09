use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crossbeam_channel::{Receiver, Sender, bounded};
use midir::{Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use virtual_midi_core::{Config, EndpointDirection, EndpointId, EndpointKind, MidiEvent};

use crate::{EndpointInfo, MidiBackend, PlatformError, PlatformStatus};

type MidiCallback = unsafe extern "C" fn(*const c_char, *const u8, usize, u64, *mut c_void);

unsafe extern "C" {
    fn vmm_windows_midi_runtime_available() -> i32;
    fn vmm_windows_midi_initialize() -> i32;
    fn vmm_windows_midi_create_virtual_device(
        name: *const c_char,
        stable_id: *const c_char,
        callback: MidiCallback,
        context: *mut c_void,
    ) -> i32;
    fn vmm_windows_midi_destroy_virtual_devices();
    fn vmm_windows_midi_send(stable_id: *const c_char, data: *const u8, length: usize) -> i32;
}

struct CallbackContext {
    sender: Sender<MidiEvent>,
    queue_overflows: Arc<AtomicU64>,
}

pub struct WindowsBackend {
    incoming_tx: Sender<MidiEvent>,
    incoming_rx: Receiver<MidiEvent>,
    contexts: Vec<Box<CallbackContext>>,
    input_connections: Vec<MidiInputConnection<()>>,
    outputs: HashMap<EndpointId, MidiOutputConnection>,
    endpoints: Vec<EndpointInfo>,
    config: Config,
    active_system_signature: Vec<(u8, String)>,
    queue_overflows: Arc<AtomicU64>,
}

unsafe extern "C" fn receive_virtual_midi(
    endpoint_id: *const c_char,
    data: *const u8,
    length: usize,
    timestamp: u64,
    context: *mut c_void,
) {
    if endpoint_id.is_null() || data.is_null() || context.is_null() {
        return;
    }
    let id = unsafe { CStr::from_ptr(endpoint_id) }.to_string_lossy();
    let bytes = unsafe { std::slice::from_raw_parts(data, length) }.to_vec();
    let context = unsafe { &*(context.cast::<CallbackContext>()) };
    if context
        .sender
        .try_send(MidiEvent::new(
            EndpointId::new(format!("virtual:{id}:in")),
            timestamp,
            bytes,
        ))
        .is_err()
    {
        context.queue_overflows.fetch_add(1, Ordering::Relaxed);
    }
}

impl WindowsBackend {
    pub fn new(capacity: usize) -> Result<Self, PlatformError> {
        if unsafe { vmm_windows_midi_initialize() } == 0 {
            return Err(PlatformError::RuntimeUnavailable(
                "Windows MIDI Services oder das versionierte App SDK ist nicht verfügbar. Setze VMM_WINDOWS_MIDI_SDK_INCLUDE beim Build auf die C++/WinRT-Includes des SDK.".into(),
            ));
        }
        let (incoming_tx, incoming_rx) = bounded(capacity);
        Ok(Self {
            incoming_tx,
            incoming_rx,
            contexts: Vec::new(),
            input_connections: Vec::new(),
            outputs: HashMap::new(),
            endpoints: Vec::new(),
            config: Config::default(),
            active_system_signature: Vec::new(),
            queue_overflows: Arc::new(AtomicU64::new(0)),
        })
    }

    fn system_signature(&self) -> Result<Vec<(u8, String)>, PlatformError> {
        let input = MidiInput::new("Virtual MIDI Master hotplug discovery")
            .map_err(|error| PlatformError::Initialization(error.to_string()))?;
        let mut signature = input
            .ports()
            .iter()
            .filter_map(|port| input.port_name(port).ok().map(|name| (port.id(), name)))
            .map(|(id, name)| (0, format!("physical:input:{id}|{name}")))
            .collect::<Vec<_>>();
        let output = MidiOutput::new("Virtual MIDI Master hotplug discovery")
            .map_err(|error| PlatformError::Initialization(error.to_string()))?;
        signature.extend(
            output
                .ports()
                .iter()
                .filter_map(|port| output.port_name(port).ok().map(|name| (port.id(), name)))
                .map(|(id, name)| (1, format!("physical:output:{id}|{name}"))),
        );
        signature.sort();
        Ok(signature)
    }

    fn connected_physical_signature(&self) -> Vec<(u8, String)> {
        let mut signature = self
            .endpoints
            .iter()
            .filter(|endpoint| endpoint.kind == EndpointKind::Physical)
            .map(|endpoint| {
                (
                    u8::from(endpoint.direction == EndpointDirection::Target),
                    format!("{}|{}", endpoint.id, endpoint.name),
                )
            })
            .collect::<Vec<_>>();
        signature.sort();
        signature
    }

    fn rebuilt_signature_is_complete(
        &self,
        mut observed: Vec<(u8, String)>,
        config: &Config,
    ) -> bool {
        for connected in self.connected_physical_signature() {
            let Some(index) = observed
                .iter()
                .position(|candidate| candidate == &connected)
            else {
                return false;
            };
            observed.swap_remove(index);
        }
        let pair_count = config
            .virtual_devices
            .iter()
            .filter(|device| device.enabled)
            .count();
        observed
            .iter()
            .filter(|(direction, _)| *direction == 0)
            .count()
            == pair_count
            && observed
                .iter()
                .filter(|(direction, _)| *direction == 1)
                .count()
                == pair_count
    }

    fn upsert_endpoint(&mut self, endpoint: EndpointInfo) {
        if let Some(existing) = self
            .endpoints
            .iter_mut()
            .find(|existing| existing.id == endpoint.id)
        {
            *existing = endpoint;
        } else {
            self.endpoints.push(endpoint);
        }
    }

    fn connect_physical(&mut self, offline_only: bool) -> Result<(), PlatformError> {
        let input_probe = MidiInput::new("Virtual MIDI Master discovery")
            .map_err(|error| PlatformError::Initialization(error.to_string()))?;
        let input_names = input_probe
            .ports()
            .iter()
            .filter_map(|port| {
                input_probe
                    .port_name(port)
                    .ok()
                    .map(|name| (port.id(), name))
            })
            .collect::<Vec<_>>();
        for (port_id, name) in input_names {
            let mut input = MidiInput::new("Virtual MIDI Master input")
                .map_err(|error| PlatformError::Initialization(error.to_string()))?;
            input.ignore(Ignore::None);
            let ports = input.ports();
            let id = EndpointId::new(format!("physical:input:{port_id}"));
            if offline_only
                && !self
                    .endpoints
                    .iter()
                    .any(|endpoint| endpoint.id == id && !endpoint.online)
            {
                continue;
            }
            let mut endpoint = EndpointInfo {
                id: id.clone(),
                name: name.clone(),
                direction: EndpointDirection::Source,
                kind: EndpointKind::Physical,
                online: false,
            };
            let Some(port) = ports.iter().find(|port| port.id() == port_id) else {
                self.upsert_endpoint(endpoint);
                continue;
            };
            let callback_id = id.clone();
            let sender = self.incoming_tx.clone();
            let queue_overflows = self.queue_overflows.clone();
            if let Ok(connection) = input.connect(
                port,
                "Virtual MIDI Master input",
                move |timestamp, data, ()| {
                    if sender
                        .try_send(MidiEvent::new(
                            callback_id.clone(),
                            timestamp,
                            data.to_vec(),
                        ))
                        .is_err()
                    {
                        queue_overflows.fetch_add(1, Ordering::Relaxed);
                    }
                },
                (),
            ) {
                self.input_connections.push(connection);
                endpoint.online = true;
            }
            self.upsert_endpoint(endpoint);
        }

        let output_probe = MidiOutput::new("Virtual MIDI Master discovery")
            .map_err(|error| PlatformError::Initialization(error.to_string()))?;
        let output_names = output_probe
            .ports()
            .iter()
            .filter_map(|port| {
                output_probe
                    .port_name(port)
                    .ok()
                    .map(|name| (port.id(), name))
            })
            .collect::<Vec<_>>();
        for (port_id, name) in output_names {
            let output = MidiOutput::new("Virtual MIDI Master output")
                .map_err(|error| PlatformError::Initialization(error.to_string()))?;
            let ports = output.ports();
            let id = EndpointId::new(format!("physical:output:{port_id}"));
            if offline_only
                && !self
                    .endpoints
                    .iter()
                    .any(|endpoint| endpoint.id == id && !endpoint.online)
            {
                continue;
            }
            let mut endpoint = EndpointInfo {
                id: id.clone(),
                name: name.clone(),
                direction: EndpointDirection::Target,
                kind: EndpointKind::Physical,
                online: false,
            };
            let Some(port) = ports.iter().find(|port| port.id() == port_id) else {
                self.upsert_endpoint(endpoint);
                continue;
            };
            if let Ok(connection) = output.connect(port, "Virtual MIDI Master output") {
                self.outputs.insert(id.clone(), connection);
                endpoint.online = true;
            }
            self.upsert_endpoint(endpoint);
        }
        Ok(())
    }

    fn create_virtual(&mut self, config: &Config) -> Result<(), PlatformError> {
        for device in config
            .virtual_devices
            .iter()
            .filter(|device| device.enabled)
        {
            let name = CString::new(device.name.as_str())
                .map_err(|_| PlatformError::Endpoint("virtual device name contains NUL".into()))?;
            let stable_id = device.id.to_string();
            let c_id = CString::new(stable_id.as_str()).expect("UUID has no NUL");
            let mut context = Box::new(CallbackContext {
                sender: self.incoming_tx.clone(),
                queue_overflows: self.queue_overflows.clone(),
            });
            if unsafe {
                vmm_windows_midi_create_virtual_device(
                    name.as_ptr(),
                    c_id.as_ptr(),
                    receive_virtual_midi,
                    (&mut *context as *mut CallbackContext).cast(),
                )
            } == 0
            {
                return Err(PlatformError::Endpoint(format!(
                    "Windows virtual device '{}' could not be created",
                    device.name
                )));
            }
            self.contexts.push(context);
            self.endpoints.push(EndpointInfo {
                id: EndpointId::new(format!("virtual:{stable_id}:in")),
                name: format!("{} MIDI In", device.name),
                direction: EndpointDirection::Source,
                kind: EndpointKind::Virtual,
                online: true,
            });
            self.endpoints.push(EndpointInfo {
                id: EndpointId::new(format!("virtual:{stable_id}:out")),
                name: format!("{} MIDI Out", device.name),
                direction: EndpointDirection::Target,
                kind: EndpointKind::Virtual,
                online: true,
            });
        }
        Ok(())
    }

    fn rebuild_once(&mut self, config: &Config) -> Result<Vec<EndpointInfo>, PlatformError> {
        unsafe { vmm_windows_midi_destroy_virtual_devices() };
        self.contexts.clear();
        self.input_connections.clear();
        self.outputs.clear();
        self.endpoints.clear();
        self.active_system_signature.clear();
        let expected_physical_signature = self.system_signature()?;
        self.connect_physical(false)?;
        if self.connected_physical_signature() != expected_physical_signature {
            return Err(PlatformError::Endpoint(
                "MIDI topology changed while physical ports were connected; retrying".into(),
            ));
        }
        self.create_virtual(config)?;
        self.config = config.clone();
        let active_system_signature = self.system_signature()?;
        if !self.rebuilt_signature_is_complete(active_system_signature.clone(), config) {
            return Err(PlatformError::Endpoint(
                "MIDI topology changed while virtual ports were created; retrying".into(),
            ));
        }
        self.active_system_signature = active_system_signature;
        Ok(self.endpoints.clone())
    }

    fn rebuild(&mut self, config: &Config) -> Result<Vec<EndpointInfo>, PlatformError> {
        let mut last_error = None;
        for _ in 0..3 {
            match self.rebuild_once(config) {
                Ok(endpoints) => return Ok(endpoints),
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.expect("the rebuild loop always runs at least once"))
    }
}

impl Drop for WindowsBackend {
    fn drop(&mut self) {
        unsafe { vmm_windows_midi_destroy_virtual_devices() };
    }
}

impl MidiBackend for WindowsBackend {
    fn reconfigure(&mut self, config: &Config) -> Result<Vec<EndpointInfo>, PlatformError> {
        if !self.endpoints.is_empty()
            && !self.active_system_signature.is_empty()
            && self.config.virtual_devices == config.virtual_devices
            && self.system_signature()? == self.active_system_signature
        {
            self.config = config.clone();
            return Ok(self.endpoints.clone());
        }
        self.rebuild(config)
    }

    fn refresh(&mut self) -> Result<Option<Vec<EndpointInfo>>, PlatformError> {
        if self.system_signature()? == self.active_system_signature {
            let offline_before = self
                .endpoints
                .iter()
                .filter(|endpoint| !endpoint.online)
                .count();
            if offline_before > 0 {
                self.connect_physical(true)?;
                let offline_after = self
                    .endpoints
                    .iter()
                    .filter(|endpoint| !endpoint.online)
                    .count();
                if offline_after < offline_before {
                    return Ok(Some(self.endpoints.clone()));
                }
            }
            return Ok(None);
        }
        let config = self.config.clone();
        self.rebuild(&config).map(Some)
    }

    fn endpoints(&self) -> Vec<EndpointInfo> {
        self.endpoints.clone()
    }
    fn try_receive(&self) -> Option<MidiEvent> {
        self.incoming_rx.try_recv().ok()
    }

    fn take_queue_overflows(&self) -> u64 {
        self.queue_overflows.swap(0, Ordering::Relaxed)
    }

    fn send(&mut self, target: &EndpointId, data: &[u8]) -> Result<(), PlatformError> {
        if let Some(output) = self.outputs.get_mut(target) {
            return output
                .send(data)
                .map_err(|error| PlatformError::Endpoint(error.to_string()));
        }
        if let Some(stable_id) = target
            .as_str()
            .strip_prefix("virtual:")
            .and_then(|id| id.strip_suffix(":out"))
        {
            let id = CString::new(stable_id).expect("endpoint id has no NUL");
            if unsafe { vmm_windows_midi_send(id.as_ptr(), data.as_ptr(), data.len()) } != 0 {
                return Ok(());
            }
        }
        Err(PlatformError::Endpoint(format!("offline target {target}")))
    }

    fn status(&self) -> PlatformStatus {
        PlatformStatus {
            platform: "windows-midi-services".into(),
            runtime_available: unsafe { vmm_windows_midi_runtime_available() } != 0,
            detail:
                "Windows MIDI Services Virtual Device transport with MIDI 1.0 compatibility ports"
                    .into(),
        }
    }
}
