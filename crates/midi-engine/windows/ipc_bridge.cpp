#include <windows.h>
#include <sddl.h>
#include <stdint.h>

namespace {
HANDLE g_instance_mutex = nullptr;
}

extern "C" int vmm_instance_lock_acquire() {
  if (g_instance_mutex) return 1;
  HANDLE handle = CreateMutexW(nullptr, FALSE,
                               L"Local\\VirtualMidiMasterEngine-4EA4770A");
  if (!handle) return 0;
  if (GetLastError() == ERROR_ALREADY_EXISTS) {
    CloseHandle(handle);
    return 0;
  }
  g_instance_mutex = handle;
  return 1;
}

extern "C" void vmm_instance_lock_release() {
  if (!g_instance_mutex) return;
  CloseHandle(g_instance_mutex);
  g_instance_mutex = nullptr;
}

extern "C" void *vmm_ipc_accept(uint8_t *buffer, uint32_t capacity,
                                  uint32_t *bytes_read) {
  PSECURITY_DESCRIPTOR descriptor = nullptr;
  SECURITY_ATTRIBUTES attributes{};
  attributes.nLength = sizeof(attributes);
  if (ConvertStringSecurityDescriptorToSecurityDescriptorW(
          L"D:P(A;;GA;;;SY)(A;;GA;;;OW)", SDDL_REVISION_1, &descriptor,
          nullptr)) {
    attributes.lpSecurityDescriptor = descriptor;
  }
  HANDLE pipe = CreateNamedPipeW(
      L"\\\\.\\pipe\\virtual-midi-master", PIPE_ACCESS_DUPLEX,
      PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT |
          PIPE_REJECT_REMOTE_CLIENTS,
      1, 1024 * 1024, 1024 * 1024, 0,
      descriptor ? &attributes : nullptr);
  if (descriptor) LocalFree(descriptor);
  if (pipe == INVALID_HANDLE_VALUE) return nullptr;
  BOOL connected = ConnectNamedPipe(pipe, nullptr) ||
                   GetLastError() == ERROR_PIPE_CONNECTED;
  if (!connected) {
    CloseHandle(pipe);
    return nullptr;
  }
  DWORD count = 0;
  if (!ReadFile(pipe, buffer, capacity, &count, nullptr)) {
    DisconnectNamedPipe(pipe);
    CloseHandle(pipe);
    return nullptr;
  }
  *bytes_read = count;
  return pipe;
}
extern "C" int vmm_ipc_reply(void *opaque, const uint8_t *data,
                              uint32_t length) {
  if (!opaque) return 0;
  HANDLE pipe = static_cast<HANDLE>(opaque);
  DWORD written = 0;
  BOOL ok = WriteFile(pipe, data, length, &written, nullptr);
  FlushFileBuffers(pipe);
  DisconnectNamedPipe(pipe);
  CloseHandle(pipe);
  return ok && written == length;
}

extern "C" int vmm_ipc_request(const uint8_t *request,
                                uint32_t request_length, uint8_t *response,
                                uint32_t response_capacity,
                                uint32_t *response_length) {
  if (!WaitNamedPipeW(L"\\\\.\\pipe\\virtual-midi-master", 5000)) return 0;
  HANDLE pipe = CreateFileW(L"\\\\.\\pipe\\virtual-midi-master",
                            GENERIC_READ | GENERIC_WRITE, 0, nullptr,
                            OPEN_EXISTING, 0, nullptr);
  if (pipe == INVALID_HANDLE_VALUE) return 0;
  DWORD written = 0;
  if (!WriteFile(pipe, request, request_length, &written, nullptr)) {
    CloseHandle(pipe);
    return 0;
  }
  DWORD read = 0;
  BOOL ok = ReadFile(pipe, response, response_capacity, &read, nullptr);
  CloseHandle(pipe);
  if (!ok && GetLastError() != ERROR_MORE_DATA) return 0;
  *response_length = read;
  return 1;
}
