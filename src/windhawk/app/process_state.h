#pragma once

namespace Functions {

// Returns true for suspended UWP processes.
// https://stackoverflow.com/a/50173965
bool IsProcessFrozen(HANDLE hProcess);

NTSTATUS CreateExecutionRequiredRequest(_In_ HANDLE ProcessHandle,
                                        _Out_ PHANDLE PowerRequestHandle);

}  // namespace Functions
