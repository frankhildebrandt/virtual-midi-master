#include "bridge.h"

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>

#include <chrono>
#include <cstring>
#include <memory>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

#if !__has_include(<winrt/Microsoft.Windows.Devices.Midi2.h>)
#error "Windows MIDI SDK headers were not found. Set VMM_WINDOWS_MIDI_SDK_INCLUDE to the generated C++/WinRT projection."
#endif

#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Foundation.Collections.h>
#include <winrt/Microsoft.Windows.Devices.Midi2.h>
#include <winrt/Microsoft.Windows.Devices.Midi2.Endpoints.Virtual.h>
#include <winrt/Microsoft.Windows.Devices.Midi2.Messages.h>
#include "Microsoft.Windows.Devices.Midi2.Initialization.hpp"

using namespace winrt::Microsoft::Windows::Devices::Midi2;
using namespace winrt::Microsoft::Windows::Devices::Midi2::Endpoints::Virtual;
using namespace winrt::Microsoft::Windows::Devices::Midi2::Messages;
using Microsoft::Windows::Devices::Midi2::Initialization::MidiDesktopAppSdkInitializer;

namespace {
struct VirtualDeviceState {
  std::string stable_id;
  vmm_midi_callback callback{};
  void *context{};
  std::mutex callback_mutex;
  MidiSession session{nullptr};
  MidiVirtualDevice device{nullptr};
  MidiEndpointConnection connection{nullptr};
  winrt::event_token message_token{};
};

std::mutex g_devices_mutex;
std::vector<std::shared_ptr<VirtualDeviceState>> g_devices;
std::once_flag g_apartment;
std::shared_ptr<MidiDesktopAppSdkInitializer> g_initializer;
std::mutex g_error_mutex;
std::string g_last_error = "Windows MIDI Services has not been initialized";
char g_error_copy[1024];

void set_last_error(std::string message) {
  std::lock_guard lock(g_error_mutex);
  g_last_error = std::move(message);
}

std::string describe_error(winrt::hresult_error const &error) {
  try {
    auto message = winrt::to_string(error.message());
    if (!message.empty()) return message;
  } catch (...) {
  }
  return "Windows MIDI Services error 0x" + std::to_string(static_cast<uint32_t>(error.code()));
}

void initialize_apartment_once() {
  std::call_once(g_apartment, [] { winrt::init_apartment(winrt::apartment_type::multi_threaded); });
}

std::string product_instance_id(const char *stable_id) {
  std::string compact;
  for (const char *cursor = stable_id; *cursor; ++cursor) {
    if (*cursor != '-') compact.push_back(*cursor);
    if (compact.size() == 16) break;
  }
  return compact.empty() ? "VMM0000000000001" : compact;
}

bool wait_for_client_endpoint(MidiVirtualDevice const &device) {
  auto association = device.AssociationId();
  for (int attempt = 0; attempt < 50; ++attempt) {
    try {
      auto client_id = MidiVirtualDeviceManager::GetAssociatedClientEndpointDeviceId(association);
      if (!client_id.empty()) return true;
    } catch (...) {
    }
    std::this_thread::sleep_for(std::chrono::milliseconds(20));
  }
  return false;
}

int initialize_runtime() {
  try {
    initialize_apartment_once();
    if (!g_initializer) {
      auto initializer = std::make_shared<MidiDesktopAppSdkInitializer>();
      if (!initializer->InitializeSdkRuntime()) {
        set_last_error(
            "Windows MIDI Services App SDK Runtime konnte nicht geladen werden. Installiere die Runtime von https://aka.ms/midi");
        return 0;
      }
      g_initializer = std::move(initializer);
    }
    if (!g_initializer->EnsureServiceAvailable()) {
      set_last_error(
          "Windows MIDI Services ist nicht verfügbar. Windows 11 24H2/25H2 mit aktivierten MIDI Services wird benötigt.");
      return 0;
    }
    if (!MidiVirtualDeviceManager::IsTransportAvailable()) {
      set_last_error("Der Virtual-Device-Transport von Windows MIDI Services ist nicht aktiv.");
      return 0;
    }
    set_last_error("Windows MIDI Services is available");
    return 1;
  } catch (winrt::hresult_error const &error) {
    set_last_error(describe_error(error));
    return 0;
  } catch (std::exception const &error) {
    set_last_error(error.what());
    return 0;
  } catch (...) {
    set_last_error("Windows MIDI Services konnte nicht initialisiert werden");
    return 0;
  }
}
} // namespace

const char *vmm_windows_midi_last_error(void) {
  std::lock_guard lock(g_error_mutex);
  strncpy_s(g_error_copy, g_last_error.c_str(), _TRUNCATE);
  return g_error_copy;
}

int vmm_windows_midi_runtime_available(void) { return initialize_runtime(); }

int vmm_windows_midi_initialize(void) { return initialize_runtime(); }

int vmm_windows_midi_create_virtual_device(const char *name,
                                            const char *stable_id,
                                            vmm_midi_callback callback,
                                            void *context) {
  if (!name || !stable_id || !callback) {
    set_last_error("virtual device name, id, or callback is missing");
    return 0;
  }
  if (!initialize_runtime()) return 0;
  try {
    auto state = std::make_shared<VirtualDeviceState>();
    state->stable_id = stable_id;
    state->callback = callback;
    state->context = context;
    auto display_name = winrt::to_hstring(name);

    MidiDeclaredEndpointInfo endpoint_info{};
    endpoint_info.Name = display_name;
    endpoint_info.ProductInstanceId = winrt::to_hstring(product_instance_id(stable_id));
    endpoint_info.HasStaticFunctionBlocks = true;
    endpoint_info.DeclaredFunctionBlockCount = 1;
    endpoint_info.SupportsMidi10Protocol = true;
    endpoint_info.SupportsMidi20Protocol = false;
    endpoint_info.SupportsReceivingJitterReductionTimestamps = false;
    endpoint_info.SupportsSendingJitterReductionTimestamps = false;
    endpoint_info.SpecificationVersionMajor = 1;
    endpoint_info.SpecificationVersionMinor = 1;

    MidiVirtualDeviceCreationConfig creation_config(
        display_name, L"Virtual MIDI Master MIDI 1.0 endpoint",
        L"Virtual MIDI Master", endpoint_info);
    creation_config.CreateOnlyUmpEndpoints(false);

    MidiFunctionBlock block;
    block.Number(0);
    block.IsActive(true);
    block.Name(display_name);
    block.FirstGroup(MidiGroup(static_cast<uint8_t>(0)));
    block.GroupCount(1);
    block.Direction(MidiFunctionBlockDirection::Bidirectional);
    block.UIHint(MidiFunctionBlockUIHint::Bidirectional);
    block.RepresentsMidi10Connection(
        MidiFunctionBlockRepresentsMidi10Connection::YesBandwidthUnrestricted);
    creation_config.FunctionBlocks().Append(block);

    state->session = MidiSession::Create(display_name);
    if (!state->session) {
      set_last_error("MidiSession::Create returned null");
      return 0;
    }
    state->device = MidiVirtualDeviceManager::CreateVirtualDevice(creation_config);
    if (!state->device) {
      set_last_error("MidiVirtualDeviceManager::CreateVirtualDevice returned null");
      return 0;
    }
    state->connection = state->session.CreateEndpointConnection(state->device.DeviceEndpointDeviceId());
    if (!state->connection) {
      set_last_error("CreateEndpointConnection returned null");
      return 0;
    }

    state->message_token = state->connection.MessageReceived(
        [state](IMidiMessageReceivedEventSource const &,
                MidiMessageReceivedEventArgs const &args) {
          try {
            std::lock_guard callback_lock(state->callback_mutex);
            if (!state->context) return;
            auto words = winrt::single_threaded_vector<uint32_t>();
            args.AppendWordsToList(words);
            auto bytes = MidiMessageConverter::ConvertSingleGroupCompleteMessageUmpWordsToMidi1Bytes(words);
            if (!bytes || bytes.Size() == 0) return;
            std::vector<uint8_t> copied(bytes.Size());
            for (uint32_t index = 0; index < bytes.Size(); ++index) copied[index] = bytes.GetAt(index);
            state->callback(state->stable_id.c_str(), copied.data(), copied.size(),
                            args.Timestamp(), state->context);
          } catch (...) {
          }
        });
    state->connection.AddMessageProcessingPlugin(state->device);
    if (!state->connection.Open()) {
      set_last_error("opening the virtual device endpoint failed");
      return 0;
    }
    if (!wait_for_client_endpoint(state->device)) {
      set_last_error(
          "virtual device opened, but the client-visible MIDI 1.0 endpoint did not appear in time");
    }

    std::lock_guard lock(g_devices_mutex);
    g_devices.push_back(state);
    return 1;
  } catch (winrt::hresult_error const &error) {
    set_last_error(describe_error(error));
    return 0;
  } catch (std::exception const &error) {
    set_last_error(error.what());
    return 0;
  } catch (...) {
    set_last_error("creating the Windows virtual MIDI device failed");
    return 0;
  }
}

void vmm_windows_midi_destroy_virtual_devices(void) {
  std::lock_guard lock(g_devices_mutex);
  for (auto &state : g_devices) {
    try {
      if (state->connection) {
        state->connection.MessageReceived(state->message_token);
        state->session.DisconnectEndpointConnection(state->connection.ConnectionId());
      }
      {
        // Wait for an in-flight callback and prevent later callbacks from
        // dereferencing the Rust-owned context during backend reconfiguration.
        std::lock_guard callback_lock(state->callback_mutex);
        state->context = nullptr;
      }
      if (state->session) state->session.Close();
    } catch (...) {
    }
  }
  g_devices.clear();
}

int vmm_windows_midi_send(const char *stable_id,
                          const uint8_t *data,
                          size_t length) {
  if (!stable_id || !data || length == 0) {
    set_last_error("MIDI send is missing an endpoint id or payload");
    return 0;
  }
  try {
    std::lock_guard lock(g_devices_mutex);
    for (auto &state : g_devices) {
      if (state->stable_id != stable_id) continue;
      auto bytes = winrt::single_threaded_vector<uint8_t>();
      for (size_t index = 0; index < length; ++index) bytes.Append(data[index]);
      auto words = MidiMessageConverter::ConvertMidi1CompleteMessageBytesToUmpWords(
          MidiGroup(static_cast<uint8_t>(0)), bytes, false);
      auto result = state->connection.SendMultipleMessagesWordList(MidiClock::Now(), words);
      if (MidiEndpointConnection::SendMessageSucceeded(result)) return 1;
      set_last_error("SendMultipleMessagesWordList failed");
      return 0;
    }
    set_last_error(std::string("no virtual device is open for ") + stable_id);
    return 0;
  } catch (winrt::hresult_error const &error) {
    set_last_error(describe_error(error));
    return 0;
  } catch (std::exception const &error) {
    set_last_error(error.what());
    return 0;
  } catch (...) {
    set_last_error("sending MIDI to the Windows virtual device failed");
    return 0;
  }
}
