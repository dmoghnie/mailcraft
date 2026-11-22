// MailCraft's phone library (apps/mailcraft-mobile/src/ios.rs), for the Swift shell.
//
// Text in is NUL-terminated UTF-8 the caller keeps valid for the call. Every string returned
// was made by the library: read it, then give it back with mailcraft_free, once. Calls that
// can fail return "" when all went well, or what went wrong.
#ifndef MAILCRAFTCORE_H
#define MAILCRAFTCORE_H

#include <stdint.h>

// Open the cache in `dir` and start; `secrets` is what the Keychain kept, a JSON object.
char *mailcraft_start(const char *dir, const char *secrets, int32_t offset, const char *zone);
// The person's action, as JSON.
char *mailcraft_send(const char *action);
// The next message (JSON), waiting up to `timeout_ms`; "" when none came. Blocks: call it
// from a thread of its own.
char *mailcraft_poll(uint64_t timeout_ms);
// The view model now, as JSON.
char *mailcraft_view(void);
// The phone's offset (seconds east of UTC) or time zone changed.
char *mailcraft_zone(int32_t offset, const char *zone);
void mailcraft_free(char *s);

#endif
