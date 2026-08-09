import { invoke } from "@tauri-apps/api/core";
import type { Config, EngineSnapshot, IpcRequest, IpcResponse } from "./types";

async function send(request: IpcRequest): Promise<IpcResponse> {
  return invoke<IpcResponse>("engine_request", { request });
}
export async function snapshot(): Promise<EngineSnapshot> {
  const response = await send({ op: "get_snapshot" });
  if (response.type === "snapshot") return response.snapshot;
  if (response.type === "error") throw new Error(response.message);
  throw new Error(`Unerwartete Engine-Antwort: ${response.type}`);
}

export async function subscribe(afterRevision: number): Promise<EngineSnapshot> {
  const response = await send({ op: "subscribe", after_revision: afterRevision });
  if (response.type === "snapshot") return response.snapshot;
  if (response.type === "error") throw new Error(response.message);
  throw new Error(`Unerwartete Engine-Antwort: ${response.type}`);
}

export async function applyConfig(config: Config): Promise<EngineSnapshot> {
  const response = await send({ op: "apply_config", config });
  if (response.type === "applied") return response.snapshot;
  if (response.type === "error") throw new Error(response.message);
  throw new Error(`Unerwartete Engine-Antwort: ${response.type}`);
}
