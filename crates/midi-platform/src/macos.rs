use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crossbeam_channel::{Receiver, Sender, bounded};
use midir::os::unix::{VirtualInput, VirtualOutput};
use midir::{Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use virtual_midi_core::{Config, EndpointDirection, EndpointId, EndpointKind, MidiEvent};

use crate::{EndpointInfo, MidiBackend, PlatformError, PlatformStatus};

pub struct MacOsBackend {
    incoming_tx: Sender<MidiEvent>,
    incoming_rx: Receiver<MidiEvent>,
    input_connections: Vec<MidiInputConnection<()>>,
    outputs: HashMap<EndpointId, MidiOutputConnection>,
    endpoints: Vec<EndpointInfo>,
    config: Config,
    active_system_signature: Vec<(u8, String)>,
    queue_overflows: Arc<AtomicU64>,
}

impl MacOsBackend {
    pub fn new(queue_capacity: usize) -> Self {
        let (incoming_tx, incoming_rx) = bounded(queue_capacity);
        Self {
            incoming_tx,
            incoming_rx,
            input_connections: Vec::new(),
            outputs: HashMap::new(),
            endpoints: Vec::new(),
            config: Config::default(),
            active_system_signature: Vec::new(),
            queue_overflows: Arc::new(AtomicU64::new(0)),
        }
    }

    fn system_signature(&self) -> Result<Vec<(u8, String)>, PlatformError> {
        let input = MidiInput::new("Virtual MIDI Master hotplug discovery")
            .map_err(|error| PlatformError::Initialization(format!("{error:?}")))?;
        let mut signature = input
            .ports()
            .iter()
            .filter_map(|port| input.port_name(port).ok().map(|name| (port.id(), name)))
            .map(|(id, name)| (0, format!("physical:input:{id}|{name}")))
            .collect::<Vec<_>>();
        let output = MidiOutput::new("Virtual MIDI Master hotplug discovery")
            .map_err(|error| PlatformError::Initialization(format!("{error:?}")))?;
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

    fn connect_physical_inputs(&mut self, offline_only: bool) -> Result<(), PlatformError> {
        let probe = MidiInput::new("Virtual MIDI Master discovery")
            .map_err(|error| PlatformError::Initialization(format!("{error:?}")))?;
        let names = probe
            .ports()
            .iter()
            .filter_map(|port| probe.port_name(port).ok().map(|name| (port.id(), name)))
            .collect::<Vec<_>>();
        for (port_id, name) in names {
            let mut input = MidiInput::new("Virtual MIDI Master input")
                .map_err(|error| PlatformError::Initialization(format!("{error:?}")))?;
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
            let tx = self.incoming_tx.clone();
            let overflows = self.queue_overflows.clone();
            match input.connect(
                port,
                &format!("Virtual MIDI Master ← {name}"),
                move |timestamp, message, ()| {
                    if tx
                        .try_send(MidiEvent::new(
                            callback_id.clone(),
                            timestamp,
                            message.to_vec(),
                        ))
                        .is_err()
                    {
                        overflows.fetch_add(1, Ordering::Relaxed);
                    }
                },
                (),
            ) {
                Ok(connection) => {
                    self.input_connections.push(connection);
                    endpoint.online = true;
                    self.upsert_endpoint(endpoint);
                }
                Err(error) => {
                    tracing_error(&format!("could not open MIDI input: {error}"));
                    self.upsert_endpoint(endpoint);
                }
            }
        }
        Ok(())
    }

    fn connect_physical_outputs(&mut self, offline_only: bool) -> Result<(), PlatformError> {
        let probe = MidiOutput::new("Virtual MIDI Master discovery")
            .map_err(|error| PlatformError::Initialization(format!("{error:?}")))?;
        let names = probe
            .ports()
            .iter()
            .filter_map(|port| probe.port_name(port).ok().map(|name| (port.id(), name)))
            .collect::<Vec<_>>();
        for (port_id, name) in names {
            let output = MidiOutput::new("Virtual MIDI Master output")
                .map_err(|error| PlatformError::Initialization(format!("{error:?}")))?;
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
            match output.connect(port, &format!("Virtual MIDI Master → {name}")) {
                Ok(connection) => {
                    self.outputs.insert(id.clone(), connection);
                    endpoint.online = true;
                    self.upsert_endpoint(endpoint);
                }
                Err(error) => {
                    tracing_error(&format!("could not open MIDI output: {error}"));
                    self.upsert_endpoint(endpoint);
                }
            }
        }
        Ok(())
    }

    fn create_virtual_devices(&mut self, config: &Config) -> Result<(), PlatformError> {
        for device in config
            .virtual_devices
            .iter()
            .filter(|device| device.enabled)
        {
            let input_id = EndpointId::new(format!("virtual:{}:in", device.id));
            let output_id = EndpointId::new(format!("virtual:{}:out", device.id));

            let mut input = MidiInput::new("Virtual MIDI Master virtual input")
                .map_err(|error| PlatformError::Initialization(format!("{error:?}")))?;
            input.ignore(Ignore::None);
            let callback_id = input_id.clone();
            let tx = self.incoming_tx.clone();
            let overflows = self.queue_overflows.clone();
            let connection = input
                .create_virtual(
                    &format!("{} MIDI In", device.name),
                    move |timestamp, message, ()| {
                        if tx
                            .try_send(MidiEvent::new(
                                callback_id.clone(),
                                timestamp,
                                message.to_vec(),
                            ))
                            .is_err()
                        {
                            overflows.fetch_add(1, Ordering::Relaxed);
                        }
                    },
                    (),
                )
                .map_err(|error| PlatformError::Endpoint(error.to_string()))?;
            self.input_connections.push(connection);
            self.endpoints.push(EndpointInfo {
                id: input_id,
                name: format!("{} MIDI In", device.name),
                direction: EndpointDirection::Source,
                kind: EndpointKind::Virtual,
                online: true,
            });

            let output = MidiOutput::new("Virtual MIDI Master virtual output")
                .map_err(|error| PlatformError::Initialization(format!("{error:?}")))?;
            let connection = output
                .create_virtual(&format!("{} MIDI Out", device.name))
                .map_err(|error| PlatformError::Endpoint(error.to_string()))?;
            self.outputs.insert(output_id.clone(), connection);
            self.endpoints.push(EndpointInfo {
                id: output_id,
                name: format!("{} MIDI Out", device.name),
                direction: EndpointDirection::Target,
                kind: EndpointKind::Virtual,
                online: true,
            });
        }
        Ok(())
    }

    fn rebuild_once(&mut self, config: &Config) -> Result<Vec<EndpointInfo>, PlatformError> {
        self.input_connections.clear();
        self.outputs.clear();
        self.endpoints.clear();
        self.active_system_signature.clear();
        let expected_physical_signature = self.system_signature()?;
        self.connect_physical_inputs(false)?;
        self.connect_physical_outputs(false)?;
        if self.connected_physical_signature() != expected_physical_signature {
            return Err(PlatformError::Endpoint(
                "MIDI topology changed while physical ports were connected; retrying".into(),
            ));
        }
        self.create_virtual_devices(config)?;
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

impl MidiBackend for MacOsBackend {
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
                self.connect_physical_inputs(true)?;
                self.connect_physical_outputs(true)?;
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

    fn take_queue_overflows(&self) -> u64 {
        self.queue_overflows.swap(0, Ordering::Relaxed)
    }

    fn try_receive(&self) -> Option<MidiEvent> {
        self.incoming_rx.try_recv().ok()
    }

    fn send(&mut self, target: &EndpointId, data: &[u8]) -> Result<(), PlatformError> {
        self.outputs
            .get_mut(target)
            .ok_or_else(|| PlatformError::Endpoint(format!("offline target {target}")))?
            .send(data)
            .map_err(|error| PlatformError::Endpoint(error.to_string()))
    }

    fn status(&self) -> PlatformStatus {
        PlatformStatus {
            platform: "macos-coremidi".into(),
            runtime_available: true,
            detail: "CoreMIDI virtual endpoints are available".into(),
        }
    }
}

fn tracing_error(message: &str) {
    eprintln!("{message}");
}
