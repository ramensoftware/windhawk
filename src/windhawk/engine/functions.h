#pragma once

namespace Functions {

// Returns true if the process enforces a binary signature policy that only
// admits Microsoft-signed or Store-signed images, which makes loading any other
// DLL into it fail and has Code Integrity log an event 3033 for the attempt.
// The audit-only variants of the policy don't count, since they leave the load
// allowed.
bool IsProcessBlockingNonMicrosoftBinaries(HANDLE hProcess);

// https://waleedassar.blogspot.com/2012/12/skipthreadattach.html
enum MyCreateRemoteThreadFlags : ULONG {
    MY_REMOTE_THREAD_CREATE_SUSPENDED = 0x01,
    MY_REMOTE_THREAD_THREAD_ATTACH_EXEMPT = 0x02,
    MY_REMOTE_THREAD_HIDE_FROM_DEBUGGER = 0x04,
    MY_REMOTE_THREAD_LOADER_WORKER = 0x10,          // since THRESHOLD
    MY_REMOTE_THREAD_SKIP_LOADER_INIT = 0x20,       // since REDSTONE2
    MY_REMOTE_THREAD_BYPASS_PROCESS_FREEZE = 0x40,  // since 19H1
};

// Using MyCreateRemoteThread instead of CreateRemoteThread allows providing
// extra flags. We use the MY_REMOTE_THREAD_THREAD_ATTACH_EXEMPT flag to reduce
// incompatibility with other processes.
HANDLE MyCreateRemoteThread(HANDLE hProcess,
                            LPTHREAD_START_ROUTINE lpStartAddress,
                            LPVOID lpParameter,
                            ULONG createFlags);

}  // namespace Functions
