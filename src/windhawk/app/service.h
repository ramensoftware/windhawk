#pragma once

namespace Service {

void Run();
bool IsRunning(bool waitIfStarting);
void Start();
void Stop(bool disableAutoStart);

}  // namespace Service
