#pragma once

namespace Functions {

bool IsSessionLoggedOn(DWORD sessionId);

// The sessions which have a user logged on. A session whose user can't be read
// is skipped, its failure logged.
std::vector<DWORD> GetLoggedOnSessionIds();

// Creates a process on the session's own user token, the interactive and
// unelevated one, with that user's environment. Needs SE_TCB_NAME, which only
// the system account has.
void CreateProcessOnSessionId(DWORD sessionId,
                              PCWSTR applicationName,
                              PWSTR commandLine);

// Like CreateProcessOnSessionId, but on the session user's full, elevated
// token. Throws when the user has none, as a standard user does.
void CreateProcessOnSessionIdElevated(DWORD sessionId,
                                      PCWSTR applicationName,
                                      PWSTR commandLine);

// Whether CreateProcessOnSessionIdElevated can serve the session, for a caller
// which picks between elevation levels ahead of the launch.
bool CanCreateProcessOnSessionIdElevated(DWORD sessionId);

// Like CreateProcessOnSessionId, but with UIAccess granted on the token, which
// comes with a raised integrity level. Needs SE_TCB_NAME, which only the system
// account has.
void CreateProcessOnSessionIdWithUiAccess(DWORD sessionId,
                                          PCWSTR applicationName,
                                          PWSTR commandLine);

// Creates a process in the caller's own session on the desktop user's token:
// as the caller when it isn't elevated, as a child of the shell process when it
// is, so that an elevated caller doesn't hand the new process rights which were
// never asked for. Throws when elevated with no shell process to take that user
// from.
void CreateProcessAsDesktopUser(PCWSTR applicationName, PWSTR commandLine);

// Creates a process in the caller's own session on the caller's own token.
// Meant for an elevated caller, so that what it creates runs elevated too.
void CreateProcessInOwnSessionElevated(PCWSTR applicationName,
                                       PWSTR commandLine);

// Like CreateProcessAsDesktopUser, but with UIAccess granted on the desktop
// user's token, which comes with a raised integrity level. Borrows the
// privileges that needs from the system account, which takes SE_DEBUG_NAME.
// Throws when the caller isn't elevated.
void CreateProcessAsDesktopUserWithUiAccess(PCWSTR applicationName,
                                            PWSTR commandLine);

}  // namespace Functions
