#include "stdafx.h"

#include "customization_session.h"
#include "functions.h"
#include "logger.h"
#include "session_private_namespace.h"

extern HINSTANCE g_hDllInst;

// static
void CustomizationSession::Start(
    bool runningFromAPC,
    bool threadAttachExempt,
    wil::unique_process_handle sessionManagerProcess,
    wil::unique_mutex_nothrow sessionMutex) {
    std::wstring semaphoreName = L"WindhawkCustomizationSessionSemaphore-pid=" +
                                 std::to_wstring(GetCurrentProcessId());
    wil::unique_semaphore semaphore(1, 1, semaphoreName.c_str());
    wil::semaphore_release_scope_exit semaphoreLock = semaphore.acquire();

    std::optional<CustomizationSession>& session = GetInstance();
    if (session) {
        throw std::logic_error(
            "Only one session is supported at any given time");
    }

    session.emplace(ConstructorSecret{}, runningFromAPC, threadAttachExempt,
                    std::move(sessionManagerProcess), std::move(sessionMutex));

    session->StartInitialized(std::move(semaphore), std::move(semaphoreLock),
                              runningFromAPC);
}

// static
DWORD CustomizationSession::GetSessionManagerProcessId() {
    HANDLE sessionManagerProcess =
        ScopedStaticSessionManagerProcess::GetInstance().value().get();

    DWORD processId = GetProcessId(sessionManagerProcess);
    THROW_LAST_ERROR_IF(processId == 0);
    return processId;
}

// static
FILETIME CustomizationSession::GetSessionManagerProcessCreationTime() {
    HANDLE sessionManagerProcess =
        ScopedStaticSessionManagerProcess::GetInstance().value().get();

    FILETIME creationTime;
    FILETIME exitTime;
    FILETIME kernelTime;
    FILETIME userTime;
    THROW_IF_WIN32_BOOL_FALSE(GetProcessTimes(sessionManagerProcess,
                                              &creationTime, &exitTime,
                                              &kernelTime, &userTime));
    return creationTime;
}

// static
bool CustomizationSession::IsEndingSoon() {
    HANDLE sessionManagerProcess =
        ScopedStaticSessionManagerProcess::GetInstance().value().get();
    return WaitForSingleObject(sessionManagerProcess, 0) == WAIT_OBJECT_0;
}

CustomizationSession::CustomizationSession(
    ConstructorSecret constructorSecret,
    bool runningFromAPC,
    bool threadAttachExempt,
    wil::unique_process_handle sessionManagerProcess,
    wil::unique_mutex_nothrow sessionMutex)
    : m_threadAttachExempt(threadAttachExempt),
      m_scopedStaticSessionManagerProcess(std::move(sessionManagerProcess)),
      m_sessionMutex(std::move(sessionMutex)),
#ifdef WH_HOOKING_ENGINE_MINHOOK
      // If runningFromAPC, no other threads should be running, skip thread
      // freeze.
      m_minHookScopeInit(runningFromAPC ? MH_FREEZE_METHOD_NONE_UNSAFE
                                        : MH_FREEZE_METHOD_FAST_UNDOCUMENTED),
#endif  // WH_HOOKING_ENGINE_MINHOOK
      m_modsManager(),
      m_newProcessInjector(m_scopedStaticSessionManagerProcess)
#ifdef WH_HOOKING_ENGINE_MINHOOK
      ,
      m_minHookScopeApply()
#endif  // WH_HOOKING_ENGINE_MINHOOK
{
    try {
        m_modsManager.AfterInit();
    } catch (const std::exception& e) {
        LOG(L"AfterInit failed: %S", e.what());
    }
}

CustomizationSession::~CustomizationSession() {
    try {
        m_modsManager.BeforeUninit();
    } catch (const std::exception& e) {
        LOG(L"BeforeUninit failed: %S", e.what());
    }
}

#ifdef WH_HOOKING_ENGINE_MINHOOK
CustomizationSession::MinHookScopeInit::MinHookScopeInit(
    MH_THREAD_FREEZE_METHOD freezeMethod) {
    MH_STATUS status = MH_Initialize();
    if (status != MH_OK) {
        LOG(L"MH_Initialize failed with %d", status);
        throw std::runtime_error("Failed to initialize MinHook");
    }

    MH_SetThreadFreezeMethod(freezeMethod);

#ifdef WH_HOOKING_ENGINE_MINHOOK_DETOURS
    MH_SetBulkOperationMode(
        /*continueOnError=*/TRUE, [](LPVOID pTarget, NTSTATUS detoursStatus) {
            LOG(L"Hooking operation failed for %p with status 0x%08X", pTarget,
                detoursStatus);
        });
#endif
}

CustomizationSession::MinHookScopeInit::~MinHookScopeInit() {
    MH_STATUS status = MH_Uninitialize();
    if (status != MH_OK) {
        LOG(L"MH_Uninitialize failed with status %d", status);
    }
}

CustomizationSession::MinHookScopeApply::MinHookScopeApply() {
    MH_STATUS status = MH_ApplyQueuedEx(MH_ALL_IDENTS);
    if (status != MH_OK) {
        LOG(L"MH_ApplyQueuedEx failed with %d", status);
    }

    MH_SetThreadFreezeMethod(MH_FREEZE_METHOD_FAST_UNDOCUMENTED);
}

CustomizationSession::MinHookScopeApply::~MinHookScopeApply() {
    MH_STATUS status = MH_DisableHook(MH_ALL_HOOKS);
    if (status != MH_OK) {
        LOG(L"MH_DisableHook failed with status %d", status);
    }
}
#endif  // WH_HOOKING_ENGINE_MINHOOK

CustomizationSession::MainLoopRunner::MainLoopRunner() noexcept {
    try {
        m_modConfigChangeNotification.emplace();
    } catch (const std::exception& e) {
        LOG(L"ModConfigChangeNotification constructor failed: %S", e.what());
    }
}

CustomizationSession::MainLoopRunner::Result
CustomizationSession::MainLoopRunner::Run(
    HANDLE sessionManagerProcess) noexcept {
    HANDLE waitHandles[] = {sessionManagerProcess,
                            m_modConfigChangeNotification
                                ? m_modConfigChangeNotification->GetHandle()
                                : nullptr};
    DWORD waitHandlesCount = m_modConfigChangeNotification ? 2 : 1;

    DWORD waitResult =
        WaitForMultipleObjects(waitHandlesCount, waitHandles, FALSE, INFINITE);
    switch (waitResult) {
        case WAIT_OBJECT_0:
            return Result::kCompleted;

        case WAIT_OBJECT_0 + 1:
            // Wait for a bit before notifying about the change, in case
            // more config changes will follow.
            if (WaitForSingleObject(sessionManagerProcess, 200) ==
                WAIT_OBJECT_0) {
                return Result::kCompleted;
            }

            return Result::kReloadModsAndSettings;
    }

    LOG(L"WaitForMultipleObjects returned %u, last error %u", waitResult,
        GetLastError());
    return Result::kError;
}

bool CustomizationSession::MainLoopRunner::ContinueMonitoring() noexcept {
    if (!m_modConfigChangeNotification) {
        return false;
    }

    try {
        m_modConfigChangeNotification->ContinueMonitoring();
    } catch (const std::exception& e) {
        LOG(L"ContinueMonitoring failed: %S", e.what());
        m_modConfigChangeNotification.reset();
        return false;
    }

    return true;
}

bool CustomizationSession::MainLoopRunner::CanRunAcrossThreads() noexcept {
    if (m_modConfigChangeNotification &&
        !m_modConfigChangeNotification->CanMonitorAcrossThreads()) {
        return false;
    }

    return true;
}

// static
std::optional<CustomizationSession>& CustomizationSession::GetInstance() {
    // Use NoDestructorIfTerminating not only for performance reasons, but also
    // because it's not safe to destruct the session when the process
    // terminates. As part of the mods unloading, we access the mods and call
    // functions such as Wh_Uninit, but at this point, the mods' global variable
    // destructors have already run, so we might be accessing destructed
    // objects. Reference: https://stackoverflow.com/a/67999399
    STATIC_INIT_ONCE(
        NoDestructorIfTerminating<std::optional<CustomizationSession>>,
        session);
    return **session;
}

void CustomizationSession::StartInitialized(
    wil::unique_semaphore semaphore,
    wil::semaphore_release_scope_exit semaphoreLock,
    bool runningFromAPC) noexcept {
    m_sessionSemaphore = std::move(semaphore);
    m_sessionSemaphoreLock = std::move(semaphoreLock);

    if (runningFromAPC) {
        m_mainLoopRunner.emplace();
        if (!m_mainLoopRunner->CanRunAcrossThreads()) {
            m_mainLoopRunner.reset();
        }

        // Bump the reference count of the module to ensure that the module will
        // stay loaded as long as the thread is executing.
        HMODULE hDllInst;
        GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                           reinterpret_cast<LPCWSTR>(g_hDllInst), &hDllInst);

        // Create a new thread with the THREAD_ATTACH_EXEMPT flag to prevent TLS
        // and DllMain callbacks from being invoked. Otherwise, they might cause
        // a crash if invoked too early, e.g. before CRT is initialized. If
        // threadAttachExempt is set, just keep running with this flag. If
        // threadAttachExempt isn't set, create a new thread without the flag
        // once some significant code runs, such as mod/config reload or unload,
        // or any mod callback.
        DWORD createThreadFlags =
            Functions::MY_REMOTE_THREAD_THREAD_ATTACH_EXEMPT;

        wil::unique_process_handle thread(Functions::MyCreateRemoteThread(
            GetCurrentProcess(),
            [](LPVOID pThis) -> DWORD {
                // Prevent the system from displaying the critical-error-handler
                // message box. A message box like this was appearing while
                // trying to load a dll in a process with the
                // ProcessSignaturePolicy mitigation, and it looked like this:
                // https://stackoverflow.com/q/38367847
                SetThreadErrorMode(SEM_FAILCRITICALERRORS, nullptr);
                auto* this_ = reinterpret_cast<CustomizationSession*>(pThis);

                if (!this_->m_mainLoopRunner) {
                    this_->m_mainLoopRunner.emplace();
                }

                if (this_->m_threadAttachExempt) {
                    this_->RunMainLoop();
                    this_->DeleteThis();
                } else {
                    this_->RunMainLoopAndDeleteThisWithThreadRecreate();
                }

                FreeLibraryAndExitThread(g_hDllInst, 0);
            },
            this, createThreadFlags));
        if (!thread) {
            LOG(L"Thread creation failed: %u", GetLastError());
            FreeLibrary(g_hDllInst);
            DeleteThis();
        }
    } else {
        // No need to create a new thread, a dedicated thread was created for us
        // before injection.
        m_mainLoopRunner.emplace();
        RunMainLoop();
        DeleteThis();
    }
}

void CustomizationSession::
    RunMainLoopAndDeleteThisWithThreadRecreate() noexcept {
    bool modConfigChanged =
        m_mainLoopRunner->Run(m_scopedStaticSessionManagerProcess) ==
        MainLoopRunner::Result::kReloadModsAndSettings;

    if (!m_mainLoopRunner->CanRunAcrossThreads()) {
        m_mainLoopRunner.reset();
    }

    LPTHREAD_START_ROUTINE routine;
    if (modConfigChanged) {
        routine = [](LPVOID pThis) -> DWORD {
            SetThreadErrorMode(SEM_FAILCRITICALERRORS, nullptr);
            auto* this_ = reinterpret_cast<CustomizationSession*>(pThis);

            if (this_->m_mainLoopRunner) {
                this_->m_mainLoopRunner->ContinueMonitoring();
            } else {
                this_->m_mainLoopRunner.emplace();
            }

            try {
                this_->m_modsManager.ReloadModsAndSettings();
            } catch (const std::exception& e) {
                LOG(L"ReloadModsAndSettings failed: %S", e.what());
            }

            this_->RunMainLoop();
            this_->DeleteThis();

            FreeLibraryAndExitThread(g_hDllInst, 0);
        };
    } else {
        routine = [](LPVOID pThis) -> DWORD {
            SetThreadErrorMode(SEM_FAILCRITICALERRORS, nullptr);
            auto* this_ = reinterpret_cast<CustomizationSession*>(pThis);

            this_->DeleteThis();

            FreeLibraryAndExitThread(g_hDllInst, 0);
        };
    }

    // Bump the reference count of the module to ensure that the module will
    // stay loaded as long as the thread is executing.
    HMODULE hDllInst;
    GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                       reinterpret_cast<LPCWSTR>(g_hDllInst), &hDllInst);

    wil::unique_process_handle thread(
        Functions::MyCreateRemoteThread(GetCurrentProcess(), routine, this, 0));
    if (!thread) {
        LOG(L"Thread creation failed: %u", GetLastError());
        FreeLibrary(g_hDllInst);
        DeleteThis();
    }
}

void CustomizationSession::RunMainLoop() noexcept {
    while (true) {
        auto result =
            m_mainLoopRunner->Run(m_scopedStaticSessionManagerProcess);
        if (result != MainLoopRunner::Result::kReloadModsAndSettings) {
            break;
        }

        m_mainLoopRunner->ContinueMonitoring();

        try {
            m_modsManager.ReloadModsAndSettings();
        } catch (const std::exception& e) {
            LOG(L"ReloadModsAndSettings failed: %S", e.what());
        }
    }

    VERBOSE(L"Exiting engine thread wait loop");
}

void CustomizationSession::DeleteThis() noexcept {
    // Make sure the semaphore is only released after the object is destroyed.
    wil::unique_semaphore semaphore = std::move(m_sessionSemaphore);
    wil::semaphore_release_scope_exit semaphoreLock =
        std::move(m_sessionSemaphoreLock);

    GetInstance().reset();
}
