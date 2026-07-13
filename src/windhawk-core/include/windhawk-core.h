/* windhawk-core.dll C ABI.
 *
 * Generated from the windhawk-core-ffi crate by cbindgen; do not edit.
 *
 * All functions use the default C calling convention and are exported by
 * exactly these undecorated names on every architecture. All char* are
 * UTF-8, NUL-terminated. Strings returned by the DLL are freed with
 * WhCoreFree; strings passed in are borrowed for the duration of the
 * call.
 */

#ifndef WINDHAWK_CORE_H
#define WINDHAWK_CORE_H

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * ABI compatibility gate; bumped only on breaking C-surface changes.
 */
#define WHCORE_ABI_VERSION 2

/**
 * Opaque session handle; a `Box<Session>` round-tripped through the ABI
 * (there is no global session table).
 */
typedef struct WhCoreSession WhCoreSession;

/**
 * Log callback: `level` is 0=error, 1=warn, 2=info; `message` is UTF-8,
 * borrowed for the duration of the call.
 */
typedef void (*WhCoreLogCallback)(void *ctx, int32_t level, const char *message);

/**
 * Event callback: `event_json` is one event document of the operation
 * `op_id`, borrowed for the duration of the call.
 */
typedef void (*WhCoreEventCallback)(void *ctx, uint64_t op_id, const char *event_json);

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

/**
 * Returns the ABI version of this DLL.
 */
int32_t WhCoreGetAbiVersion(void);

/**
 * Returns static info: `{"contractVersion": "...", "coreVersion": "..."}`.
 * Free with `WhCoreFree`.
 */
char *WhCoreGetInfoJson(void);

/**
 * Creates a session from a UTF-8 JSON config document. On success returns 0
 * and sets `*out_session`. On failure returns nonzero and, when
 * `out_error_json` is non-null, sets it to an error envelope (free with
 * `WhCoreFree`).
 */
int32_t WhCoreSessionCreate(const char *config_json,
                            WhCoreLogCallback log_cb,
                            void *log_ctx,
                            WhCoreEventCallback event_cb,
                            void *event_ctx,
                            struct WhCoreSession **out_session,
                            char **out_error_json);

/**
 * Destroys a session: blocks until in-flight synchronous calls return and
 * async operations are canceled and joined. No callbacks fire after this
 * returns. Null is a no-op.
 */
void WhCoreSessionDestroy(struct WhCoreSession *session);

/**
 * Synchronous command. Returns a response envelope; never returns null
 * for valid (non-null) arguments. Free with `WhCoreFree`.
 */
char *WhCoreInvoke(struct WhCoreSession *session, const char *request_json);

/**
 * Stateless synchronous command: a session-free transport serving only the
 * pure helpers (`parseModSource`, `appendToModIdAndName`, `getCompileFlags`).
 * Needs no app root, so it lets a consumer parse a `.wh.cpp` with no Windhawk
 * environment. Returns a response envelope; never returns null for a non-null
 * request. A storage-bearing command is rejected with INVALID_REQUEST. Free
 * with `WhCoreFree`.
 */
char *WhCoreInvokeStateless(const char *request_json);

/**
 * Asynchronous command. On success returns a nonzero operation id; events
 * arrive via the session event callback. On failure returns 0 and, when
 * `out_error_json` is non-null, sets it to an error envelope (free with
 * `WhCoreFree`); no events are emitted for failed starts.
 */
uint64_t WhCoreInvokeAsync(struct WhCoreSession *session,
                           const char *request_json,
                           char **out_error_json);

/**
 * Cooperative cancel. Returns 0 if the operation was found and signaled;
 * nonzero if the id is unknown or already terminal (a harmless no-op).
 */
int32_t WhCoreCancel(struct WhCoreSession *session, uint64_t op_id);

/**
 * Frees any `char*` returned by this DLL. Null is a no-op.
 */
void WhCoreFree(char *p);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* WINDHAWK_CORE_H */
