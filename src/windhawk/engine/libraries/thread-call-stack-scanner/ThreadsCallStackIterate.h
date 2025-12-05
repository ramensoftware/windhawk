#pragma once

BOOL ThreadsCallStackInitialize();

typedef BOOL(*ThreadCallStackIterCallback)(
    HANDLE threadHandle,
    void* stackFrameAddress,
    void* userData);

// Iterates over the call stacks of all threads and calls the callback for each
// stack frame address. The callback should return TRUE to continue iterating
// or FALSE to stop iterating. The callback might be called from a different
// thread than the one that called this function.
//
// The function will suspend all threads and resume them after the iteration is
// done. Therefore, the callback must be careful not to acquire any locks,
// including indirectly by e.g. using the process heap.
BOOL ThreadsCallStackIterate(
    ThreadCallStackIterCallback callback,
    void* userData,
    DWORD timeout);

void ThreadsCallStackCleanup();
