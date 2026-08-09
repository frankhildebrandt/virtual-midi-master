export type EndpointId = string;
export type EndpointDirection = "source" | "target";
export type EndpointKind = "physical" | "virtual";
export type MessageType =
  | "pitch_bend"
  | "poly_aftertouch"
  | "channel_aftertouch"
  | "midi_clock"
  | "system_realtime"
  | "sys_ex";

export interface VirtualDevice { id: string; name: string; enabled: boolean }
export interface EndpointInfo {
  id: EndpointId;
  name: string;
  direction: EndpointDirection;
  kind: EndpointKind;
  online: boolean;
}
export type TransformRule =
  | { type: "drop_message_type"; message_type: MessageType }
  | { type: "map_channel"; from: number | null; to: number }
  | { type: "map_control_change"; from: number; to: number; channel: number | null };
export interface Route {
  id: string;
  source: EndpointId;
  target: EndpointId;
  enabled: boolean;
  rules: TransformRule[];
}
export interface Config {
  version: number;
  virtual_devices: VirtualDevice[];
  known_endpoints: Array<{ id: string; direction: EndpointDirection; kind: EndpointKind; last_known_name: string }>;
  routes: Route[];
  autostart: boolean;
}
export interface MonitorEvent {
  event: { source: string; timestamp_micros: number; data: number[] };
  targets: number;
}
export interface EngineSnapshot {
  revision: number;
  running: boolean;
  config: Config;
  endpoints: EndpointInfo[];
  platform: { platform: string; runtime_available: boolean; detail: string };
  stats: {
    received: number;
    sent: number;
    send_errors: number;
    queue_overflows: number;
    last_internal_latency_micros: number;
  };
  monitor: MonitorEvent[];
  logs: string[];
  last_error: string | null;
}

export type IpcRequest =
  | { op: "get_snapshot" }
  | { op: "subscribe"; after_revision: number }
  | { op: "get_config" }
  | { op: "validate_config"; config: Config }
  | { op: "apply_config"; config: Config };

export type IpcResponse =
  | { type: "snapshot"; snapshot: EngineSnapshot }
  | { type: "config"; config: Config }
  | { type: "valid" }
  | { type: "applied"; snapshot: EngineSnapshot }
  | { type: "ack" }
  | { type: "error"; message: string };
