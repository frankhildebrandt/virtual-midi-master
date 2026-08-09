#include "bridge.h"
#include <windows.h>
#include <memory>
#include <mutex>
#include <string>
#include <vector>

#if __has_include(<winrt/Windows.Devices.Midi2.h>)
#define VMM_HAS_WINDOWS_MIDI_SDK 1
#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Foundation.Collections.h>
#include <winrt/Windows.Devices.Midi2.h>
#include <winrt/Windows.Devices.Midi2.Enumeration.h>
#include <winrt/Windows.Devices.Midi2.Transports.Virtual.h>
#include <winrt/Windows.Devices.Midi2.Utilities.Messages.h>

using namespace winrt::Windows::Devices::Midi2;
using namespace winrt::Windows::Devices::Midi2::Enumeration;
using namespace winrt::Windows::Devices::Midi2::Transports::Virtual;
using namespace winrt::Windows::Devices::Midi2::Utilities::Messages;

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

void initialize_apartment_once() {
  std::call_once(g_apartment, [] { winrt::init_apartment(winrt::apartment_type::multi_threaded); });
}
} // namespace
#endif

int vmm_windows_midi_runtime_available(void) {
#if VMM_HAS_WINDOWS_MIDI_SDK
  try {
    initialize_apartment_once();
    return MidiApi::EnsureServiceAvailable() ? 1 : 0;
  } catch (...) {
    return 0;
  }
#else
  return 0;
#endif
}

int vmm_windows_midi_initialize(void) {
  return vmm_windows_midi_runtime_available();
}

int vmm_windows_midi_create_virtual_device(const char *name,
                                            const char *stable_id,
                                            vmm_midi_callback callback,
                                            void *context) {
#if VMM_HAS_WINDOWS_MIDI_SDK
  if (!name || !stable_id || !callback) return 0;
  try {
    auto state = std::make_shared<VirtualDeviceState>();
    state->stable_id = stable_id;
    state->callback = callback;
    state->context = context;
    auto display_name = winrt::to_hstring(name);

    MidiDeclaredEndpointInfo endpoint_info;
    endpoint_info.Name(display_name);
    endpoint_info.ProductInstanceId(winrt::to_hstring(stable_id));
    endpoint_info.HasStaticFunctionBlocks(true);
    endpoint_info.SupportsMidi10Protocol(true);
    endpoint_info.SupportsMidi20Protocol(false);
    endpoint_info.SupportsReceivingJitterReductionTimestamps(false);
    endpoint_info.SupportsSendingJitterReductionTimestamps(false);
    endpoint_info.SpecificationVersionMajor(1);
    endpoint_info.SpecificationVersionMinor(1);

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
    block.RepresentsMidi10Connection(
        MidiFunctionBlockRepresentsMidi10Connection::YesBandwidthUnrestricted);
    creation_config.FunctionBlocks().Append(block);

    state->session = MidiSession::Create(display_name);
    if (!state->session) return 0;
    state->device = MidiVirtualDeviceManager::CreateVirtualDevice(creation_config);
    if (!state->device) return 0;
    state->connection = state->session.CreateEndpointConnection(state->device.DeviceEndpointDeviceId());
    if (!state->connection) return 0;

    state->message_token = state->connection.MessageReceived(
        [state](IMidiMessageReceivedEventSource const &,
                MidiMessageReceivedEventArgs const &args) {
          try {
            std::lock_guard callback_lock(state->callback_mutex);
            if (!state->context) return;
            auto words = winrt::single_threaded_vector<uint32_t>();
            args.AppendWordsToList(words);
            auto bytes = MidiMessageConverter::ConvertSingleGroupCompleteMessageUmpWordsToMidi1Bytes(words);
            std::vector<uint8_t> copied(bytes.Size());
            for (uint32_t index = 0; index < bytes.Size(); ++index) copied[index] = bytes.GetAt(index);
            state->callback(state->stable_id.c_str(), copied.data(), copied.size(),
                            args.Timestamp(), state->context);
          } catch (...) {
          }
        });
    state->connection.AddMessageProcessingPlugin(state->device);
    if (!state->connection.Open()) return 0;

    std::lock_guard lock(g_devices_mutex);
    g_devices.push_back(state);
    return 1;
  } catch (...) {
    return 0;
  }
#else
  (void)name; (void)stable_id; (void)callback; (void)context;
  return 0;
#endif
}

void vmm_windows_midi_destroy_virtual_devices(void) {
#if VMM_HAS_WINDOWS_MIDI_SDK
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
#endif
}

int vmm_windows_midi_send(const char *stable_id,
                          const uint8_t *data,
                          size_t length) {
#if VMM_HAS_WINDOWS_MIDI_SDK
  if (!stable_id || !data || length == 0) return 0;
  try {
    std::lock_guard lock(g_devices_mutex);
    for (auto &state : g_devices) {
      if (state->stable_id != stable_id) continue;
      auto bytes = winrt::single_threaded_vector<uint8_t>();
      for (size_t index = 0; index < length; ++index) bytes.Append(data[index]);
      auto words = MidiMessageConverter::ConvertMidi1CompleteMessageBytesToUmpWords(
          MidiGroup(static_cast<uint8_t>(0)), bytes, false);
      auto result = state->connection.SendMultipleMessagesWordList(MidiClock::Now(), words);
      return MidiEndpointConnection::SendMessageSucceeded(result) ? 1 : 0;
    }
    return 0;
  } catch (...) {
    return 0;
  }
#else
  (void)stable_id; (void)data; (void)length;
  return 0;
#endif
}
