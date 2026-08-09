#pragma once
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

int vmm_windows_midi_runtime_available(void);
typedef void (*vmm_midi_callback)(const char *endpoint_id,
                                  const uint8_t *data,
                                  size_t length,
                                  uint64_t timestamp,
                                  void *context);
int vmm_windows_midi_initialize(void);
int vmm_windows_midi_create_virtual_device(const char *name,
                                            const char *stable_id,
                                            vmm_midi_callback callback,
                                            void *context);
void vmm_windows_midi_destroy_virtual_devices(void);
int vmm_windows_midi_send(const char *stable_id,
                          const uint8_t *data,
                          size_t length);

#ifdef __cplusplus
}
#endif
