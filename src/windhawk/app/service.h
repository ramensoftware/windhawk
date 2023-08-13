#pragma once

namespace Service {

void Run();
bool IsRunning();
void Start();
void Stop(bool disableAutoStart);

}  // namespace Service
