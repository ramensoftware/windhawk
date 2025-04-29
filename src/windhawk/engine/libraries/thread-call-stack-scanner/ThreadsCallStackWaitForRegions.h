#pragma once

typedef struct {
    DWORD_PTR address;
    DWORD_PTR size;
} ThreadCallStackRegionInfo;

#ifdef __cplusplus
extern "C" {
#endif

// Iterates over the call stacks of all threads and waits until no address is
// within any of the specified regions. Can be used to wait for a specific
// module to stop executing in order to safely unload it.
BOOL ThreadsCallStackWaitForRegions(
    const ThreadCallStackRegionInfo* regionInfos,
    DWORD regionInfosCount,
    DWORD maxIterations,
    DWORD timeoutPerIteration);

#ifdef __cplusplus
}
#endif
