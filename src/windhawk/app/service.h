#pragma once

namespace Service {

void Run();
bool IsRunning(bool waitIfStarting);

// Starts the service, and has it open the UI in the given logon session when
// one is named. Returns false if the service was already running, in which case
// the session id had no effect.
bool Start(std::optional<DWORD> runUiSessionId = std::nullopt);

void Stop(bool disableAutoStart);

}  // namespace Service
