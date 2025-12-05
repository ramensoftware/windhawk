#include <windows.h>

#include "ThreadsCallStackWaitForRegions.h"

#include "ThreadsCallStackIterate.h"

typedef struct {
    const ThreadCallStackRegionInfo* regionInfos;
    DWORD regionInfosCount;
    BOOL found;
} ThreadCallStackIterateParam;

static BOOL ThreadCallStackIterateProc(HANDLE threadHandle, void* stackFrameAddress, void* userData) {
    ThreadCallStackIterateParam* param = (ThreadCallStackIterateParam*)userData;

    const ThreadCallStackRegionInfo* regionInfos = param->regionInfos;
    DWORD regionInfosCount = param->regionInfosCount;

    for (DWORD i = 0; i < regionInfosCount; i++) {
        DWORD_PTR address = regionInfos[i].address;
        DWORD_PTR size = regionInfos[i].size;
        if ((DWORD_PTR)stackFrameAddress >= address &&
            (DWORD_PTR)stackFrameAddress < address + size) {
            param->found = TRUE;
            return FALSE; // Stop iterating
        }
    }

    return TRUE; // Continue iterating
}

BOOL ThreadsCallStackWaitForRegions(
    const ThreadCallStackRegionInfo* regionInfos,
    DWORD regionInfosCount,
    DWORD maxIterations,
    DWORD timeoutPerIteration) {
    ThreadCallStackIterateParam param = {
        .regionInfos = regionInfos,
        .regionInfosCount = regionInfosCount,
    };
    BOOL result = FALSE;

    ThreadsCallStackInitialize();

    for (DWORD i = 0; i < maxIterations; i++) {
        DWORD startTime = GetTickCount();

        param.found = FALSE;
        if (ThreadsCallStackIterate(ThreadCallStackIterateProc, &param, timeoutPerIteration) && !param.found) {
            result = TRUE;
            break;
        }

        if (i < maxIterations - 1) {
            DWORD elapsedTime = GetTickCount() - startTime;
            if (elapsedTime < timeoutPerIteration) {
                Sleep(timeoutPerIteration - elapsedTime);
            }
        }
    }

    ThreadsCallStackCleanup();

    return result;
}
