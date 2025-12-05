#include <phnt/phnt_windows.h>
#include <phnt/phnt.h>

#include "ThreadsCallStackIterate.h"

#include "Thread.h"
#include "Memory.h"

#if defined(_X86_)
#define CONTEXT_PC Eip
#define CONTEXT_SP Esp
#elif defined(_AMD64_)
#define CONTEXT_PC Rip
#define CONTEXT_SP Rsp
#elif defined(_ARM64_)
#define CONTEXT_PC Pc
#define CONTEXT_SP Sp
#endif

#if defined(_AMD64_) || defined(_ARM64_)

static BOOL ThreadCallStackIterate(
    HANDLE threadHandle,
    ThreadCallStackIterCallback callback,
    void* userData,
    BOOL* abort) {
    CONTEXT context;
    context.ContextFlags = CONTEXT_ALL;
    NTSTATUS status = NtGetContextThread(threadHandle, &context);
    if (!NT_SUCCESS(status)) {
        // Continue to the next thread if we can't get the context.
        return TRUE;
    }

    if (*abort || !callback(threadHandle, (void*)context.CONTEXT_PC, userData)) {
        // If aborted or the callback returns FALSE, we stop iterating.
        return FALSE;
    }

    THREAD_BASIC_INFORMATION threadInfo;
    if (!NT_SUCCESS(NtQueryInformationThread(
        threadHandle,
        ThreadBasicInformation,
        &threadInfo,
        sizeof(threadInfo),
        NULL
    ))) {
        // Continue to the next thread if we can't get the thread information.
        return TRUE;
    }

    DWORD64 stackBase = (DWORD64)threadInfo.TebBaseAddress->NtTib.StackBase;
    DWORD64 stackLimit = (DWORD64)threadInfo.TebBaseAddress->NtTib.StackLimit;

    DWORD64 lastStackLimit = stackLimit;

    BOOL firstIteration = TRUE;

    // References:
    // http://www.nynaeve.net/Code/StackWalk64.cpp
    // https://blog.s-schoener.com/2025-01-24-stack-walking-generated-code/
    // Implementation references:
    // https://chromium.googlesource.com/chromium/src/base/+/refs/heads/main/profiler/native_unwinder_win.cc
    // https://chromium.googlesource.com/chromium/src/base/+/refs/heads/main/profiler/win32_stack_frame_unwinder.cc
    // https://github.com/dotnet/runtime/blob/15bcd62bb240635f8f5ce4b7a4b3a07bb65e5ac3/src/coreclr/vm/stackwalk.cpp#L470
    while (TRUE) {
        DWORD64 imageBase;
        RUNTIME_FUNCTION* function = RtlLookupFunctionEntry(context.CONTEXT_PC, &imageBase, NULL);

        DWORD64 prevPc = context.CONTEXT_PC;
        DWORD64 prevSp = context.CONTEXT_SP;

        // If there is no function entry, then this is a leaf function.
        if (!function) {
            if (!firstIteration) {
                // In theory we shouldn't get here, as it means we've
                // encountered a function without unwind information below the
                // top of the stack, which is forbidden by the Microsoft x64
                // calling convention.
                break;
            }

#if defined(_AMD64_)
            // For X64, return address is at RSP.
            context.Rip = *(DWORD64*)context.Rsp;
            context.Rsp += sizeof(DWORD64);
#elif defined(_ARM64_)
            // For leaf function on Windows ARM64, return address is at LR(X30).
            // Add CONTEXT_UNWOUND_TO_CALL flag to avoid unwind ambiguity for
            // tailcall on ARM64, because padding after tailcall is not
            // guaranteed.
            context.Pc = context.Lr;
            context.ContextFlags |= CONTEXT_UNWOUND_TO_CALL;
#else
#error Unsupported Windows 64-bit Architecture
#endif
        } else {
            void* handlerData;
            DWORD64 establisherFrame;
            RtlVirtualUnwind(UNW_FLAG_NHANDLER, imageBase, context.CONTEXT_PC, function, &context, &handlerData, &establisherFrame, NULL);
        }

        if (context.CONTEXT_PC == 0) {
            break;
        }

        if (context.CONTEXT_PC == prevPc && context.CONTEXT_SP == prevSp) {
            // We are likely in a loop, stop iterating.
            break;
        }

        // Check if the stack pointer is within the stack limits.
        if (context.CONTEXT_SP < lastStackLimit ||
            context.CONTEXT_SP > stackBase - sizeof(DWORD64)) {
            // Stack pointer is out of bounds, stop iterating.
            break;
        }

        if (*abort || !callback(threadHandle, (void*)context.CONTEXT_PC, userData)) {
            // If aborted or the callback returns FALSE, we stop iterating.
            return FALSE;
        }

#if defined(_ARM64_)
        // The stack pointer can be re-used by ARM64 leaf frames. Example call
        // stack excerpt:
        //
        // 0:000> k
        // ...
        // 2c 0000007c`1792ed30 00007ff8`e0108014     ntdll!KiUserCallbackDispatcherReturn
        // 2d 0000007c`1792eda0 00007ff8`e2c6bd48     win32u!NtUserPeekMessage+0x4
        // 2e 0000007c`1792eda0 00007ff8`e2c64410     user32!_PeekMessage+0x30
        // 2f 0000007c`1792ee10 00007ff8`e2c648c0     user32!DialogBox2+0x1c8
        // ...
        lastStackLimit = context.CONTEXT_SP;
#else
        // For x64, the stack pointer should never be re-used.
        lastStackLimit = context.CONTEXT_SP + sizeof(DWORD64);
#endif

        firstIteration = FALSE;
    }

    return TRUE;
}

typedef struct {
    ThreadCallStackIterCallback callback;
    void* userData;
    BOOL abort;
    HANDLE eventWorkerReady;
    HANDLE eventWorkerStart;
    HANDLE eventWorkerDone;
    HANDLE* suspendedHandles;
    ULONG suspendedHandleCount;
} WorkerThreadParam;

static DWORD CALLBACK WorkerThread(LPVOID lpParameter) {
    WorkerThreadParam* param = (WorkerThreadParam*)lpParameter;

    ThreadCallStackIterCallback callback = param->callback;
    void* userData = param->userData;
    BOOL* abort = &param->abort;
    HANDLE eventWorkerReady = param->eventWorkerReady;
    HANDLE eventWorkerStart = param->eventWorkerStart;
    HANDLE eventWorkerDone = param->eventWorkerDone;

    SetEvent(eventWorkerReady);
    WaitForSingleObject(eventWorkerStart, INFINITE);

    HANDLE* suspendedHandles = param->suspendedHandles;
    ULONG suspendedHandleCount = param->suspendedHandleCount;

    for (ULONG i = 0; i < suspendedHandleCount; i++) {
        if (!ThreadCallStackIterate(suspendedHandles[i], callback, userData, abort)) {
            break;
        }
    }

    SetEvent(eventWorkerDone);

    return 0;
}

static BOOL ThreadsCallStackIterateImpl(
    ThreadCallStackIterCallback callback,
    void* userData,
    DWORD timeout) {
    BOOL result = FALSE;
    HANDLE workerThread = NULL;
    WorkerThreadParam workerThreadParam = {
        .callback = callback,
        .userData = userData,
        .abort = FALSE,
        .eventWorkerReady = CreateEvent(NULL, FALSE, FALSE, NULL),
        .eventWorkerStart = CreateEvent(NULL, FALSE, FALSE, NULL),
        .eventWorkerDone = CreateEvent(NULL, FALSE, FALSE, NULL),
    };

    if (!workerThreadParam.eventWorkerReady ||
        !workerThreadParam.eventWorkerStart ||
        !workerThreadParam.eventWorkerDone) {
        goto exit;
    }

    DWORD workerThreadId;
    workerThread = CreateThread(
        NULL, // default security attributes
        0,    // default stack size
        WorkerThread,
        &workerThreadParam,
        0,    // default creation flags
        &workerThreadId
    );
    if (!workerThread) {
        goto exit;
    }

    WaitForSingleObject(workerThreadParam.eventWorkerReady, INFINITE);

    // Do the bare minimum when threads are suspended to avoid deadlocks.
    // For that reason, the worker thread is created before that.
    NTSTATUS status = threadscan_thread_suspend(
        &workerThreadParam.suspendedHandles, &workerThreadParam.suspendedHandleCount, workerThreadId);
    if (!NT_SUCCESS(status)) {
        workerThreadParam.abort = TRUE;
        workerThreadParam.suspendedHandles = NULL;
        workerThreadParam.suspendedHandleCount = 0;
    }

    SetEvent(workerThreadParam.eventWorkerStart);

    // There are mainly two reasons for a timeout:
    // * The callback is taking too long for the given timeout.
    // * One of the suspended threads is holding a lock which prevents stack walking, see:
    //   https://devblogs.microsoft.com/oldnewthing/20250411-00/?p=111066
    //   Once threads are resumed, it should be able to proceed.
    if (!workerThreadParam.abort &&
        WaitForSingleObject(workerThreadParam.eventWorkerDone, timeout) != WAIT_OBJECT_0) {
        workerThreadParam.abort = TRUE;
    }

    if (!workerThreadParam.abort) {
        result = TRUE;
    }

    if (workerThreadParam.suspendedHandles) {
        threadscan_thread_resume(workerThreadParam.suspendedHandles, workerThreadParam.suspendedHandleCount);
    }

    WaitForSingleObject(workerThread, INFINITE);

exit:
    if (workerThreadParam.suspendedHandles) {
        threadscan_thread_free(workerThreadParam.suspendedHandles, workerThreadParam.suspendedHandleCount);
    }

    if (workerThread) {
        CloseHandle(workerThread);
    }

    if (workerThreadParam.eventWorkerReady) {
        CloseHandle(workerThreadParam.eventWorkerReady);
    }

    if (workerThreadParam.eventWorkerStart) {
        CloseHandle(workerThreadParam.eventWorkerStart);
    }

    if (workerThreadParam.eventWorkerDone) {
        CloseHandle(workerThreadParam.eventWorkerDone);
    }

    return result;
}

#elif defined(_X86_)

static BOOL ThreadCallStackIterate(
    HANDLE threadHandle,
    ThreadCallStackIterCallback callback,
    void* userData) {
    CONTEXT context;
    context.ContextFlags = CONTEXT_CONTROL;
    NTSTATUS status = NtGetContextThread(threadHandle, &context);
    if (!NT_SUCCESS(status)) {
        // Continue to the next thread if we can't get the context.
        return TRUE;
    }

    if (!callback(threadHandle, (void*)context.Eip, userData)) {
        // If the callback returns FALSE, we stop iterating.
        return FALSE;
    }

    THREAD_BASIC_INFORMATION threadInfo;
    if (!NT_SUCCESS(NtQueryInformationThread(
        threadHandle,
        ThreadBasicInformation,
        &threadInfo,
        sizeof(threadInfo),
        NULL
    ))) {
        // Continue to the next thread if we can't get the thread information.
        return TRUE;
    }

    DWORD stackBase = (DWORD)threadInfo.TebBaseAddress->NtTib.StackBase;
    DWORD stackLimit = (DWORD)threadInfo.TebBaseAddress->NtTib.StackLimit;

    DWORD lastStackLimit = stackLimit;

    // Walk the stack using the frame-pointer stored in the RBP register.
    // NOTE: Requires the binary to be compiled with frame-pointers.
    struct frame {
        struct frame* prev;
        DWORD retAddr;
    };

    struct frame* frame = (struct frame*)context.Ebp;

    while ((DWORD)frame >= lastStackLimit &&
        (DWORD)frame <= stackBase - sizeof(struct frame)) {
        lastStackLimit = (DWORD)frame + sizeof(struct frame);

        DWORD callpc = frame->retAddr;
        if (callpc == 0) {
            break;
        }

        if (!callback(threadHandle, (void*)callpc, userData)) {
            // If the callback returns FALSE, we stop iterating.
            return FALSE;
        }

        frame = frame->prev;
    }

    return TRUE;
}

static BOOL ThreadsCallStackIterateImpl(
    ThreadCallStackIterCallback callback,
    void* userData,
    DWORD timeout) {
    BOOL result = TRUE;
    DWORD startTime = GetTickCount();
    HANDLE* suspendedHandles = NULL;
    ULONG suspendedHandleCount = 0;
    NTSTATUS status = threadscan_thread_suspend(&suspendedHandles, &suspendedHandleCount, 0);
    if (NT_SUCCESS(status) && suspendedHandles) {
        for (ULONG i = 0; i < suspendedHandleCount; i++) {
            if (!ThreadCallStackIterate(suspendedHandles[i], callback, userData)) {
                // If the callback returns FALSE, we stop iterating.
                break;
            }

            DWORD elapsedTime = GetTickCount() - startTime;
            if (elapsedTime >= timeout) {
                result = FALSE;
                break;
            }
        }

        threadscan_thread_resume(suspendedHandles, suspendedHandleCount);
    }

    return result;
}

#endif

BOOL ThreadsCallStackInitialize() {
    return threadscan_memory_initialize();
}

BOOL ThreadsCallStackIterate(
    ThreadCallStackIterCallback callback,
    void* userData,
    DWORD timeout) {
    return ThreadsCallStackIterateImpl(callback, userData, timeout);
}

void ThreadsCallStackCleanup() {
    threadscan_memory_uninitialize();
}
