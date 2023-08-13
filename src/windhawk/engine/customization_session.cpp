#include "stdafx.h"

#include "customization_session.h"
#include "functions.h"
#include "logger.h"
#include "session_private_namespace.h"
#include "storage_manager.h"

extern HINSTANCE g_hDllInst;

// static
void CustomizationSession::Start(bool runningFromAPC,
                                 bool threadAttachExempt,
                                 HANDLE sessionManagerProcess,
                                 HANDLE sessionMutex) {
    std::wstring semaphoreName = L"WindhawkCustomizationSessionSemaphore-pid=" +
                                 std::to_wstring(GetCurrentProcessId());
    wil::unique_semaphore semaphore(1, 1, semaphoreName.c_str());
    wil::semaphore_release_scope_exit semaphoreLock = semaphore.acquire();

    std::optional<CustomizationSession>& session = GetInstance();
    if (session) {
        throw std::logic_error(
            "Only one session is supported at any given time");
    }

    // From this point on, we acquire the handles. Handle all exceptions here.
    try {
        session.emplace(ConstructorSecret{}, runningFromAPC, threadAttachExempt,
                        sessionManagerProcess, sessionMutex);

        session->StartInitialized(std::move(semaphore),
                                  std::move(semaphoreLock), runningFromAPC);
    } catch (const std::exception& e) {
        LOG(L"%S", e.what());
    }
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

CustomizationSession::CustomizationSession(ConstructorSecret constructorSecret,
                                           bool runningFromAPC,
                                           bool threadAttachExempt,
                                           HANDLE sessionManagerProcess,
                                           HANDLE sessionMutex) noexcept
    : m_threadAttachExempt(threadAttachExempt),
      m_scopedStaticSessionManagerProcess(sessionManagerProcess),
      m_sessionMutex(sessionMutex),
      // If runningFromAPC, no other threads should be running, skip thread
      // freeze.
      m_minHookScopeInit(runningFromAPC ? MH_FREEZE_METHOD_NONE_UNSAFE
                                        : MH_FREEZE_METHOD_FAST_UNDOCUMENTED),
      m_modsManager(),
      m_newProcessInjector(sessionManagerProcess),
      m_minHookScopeApply() {
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

CustomizationSession::MinHookScopeInit::MinHookScopeInit(
    MH_THREAD_FREEZE_METHOD freezeMethod) {
    MH_STATUS status = MH_Initialize();
    if (status != MH_OK) {
        LOG(L"MH_Initialize failed with %d", status);
        throw std::runtime_error("Failed to initialize MinHook");
    }

    MH_SetThreadFreezeMethod(freezeMethod);
}

CustomizationSession::MinHookScopeInit::~MinHookScopeInit() {
    MH_STATUS status = MH_Uninitialize();
    if (status == MH_ERROR_NOT_INITIALIZED) {
        // That's OK.
    } else if (status != MH_OK) {
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
    MH_STATUS status = MH_Uninitialize();
    if (status != MH_OK) {
        LOG(L"MH_Uninitialize failed with status %d", status);
    }
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

                if (this_->m_threadAttachExempt) {
                    this_->Run();
                    this_->DeleteThis();
                } else {
                    this_->RunAndDeleteThisWithThreadRecreate();
                }

                FreeLibraryAndExitThread(g_hDllInst, 0);
            },
            this, Functions::MY_REMOTE_THREAD_THREAD_ATTACH_EXEMPT));
        if (!thread) {
            LOG(L"Thread creation failed: %u", GetLastError());
            FreeLibrary(g_hDllInst);
            DeleteThis();
        }
    } else {
        // No need to create a new thread, a dedicated thread was created for us
        // before injection.
        Run();
        DeleteThis();
    }
}

void CustomizationSession::RunAndDeleteThisWithThreadRecreate() noexcept {
    bool modConfigChanged;
    Run(&modConfigChanged);

    LPTHREAD_START_ROUTINE routine;
    if (modConfigChanged) {
        routine = [](LPVOID pThis) -> DWORD {
            SetThreadErrorMode(SEM_FAILCRITICALERRORS, nullptr);
            auto* this_ = reinterpret_cast<CustomizationSession*>(pThis);

            try {
                this_->m_modsManager.ReloadModsAndSettings();
            } catch (const std::exception& e) {
                LOG(L"ReloadModsAndSettings failed: %S", e.what());
            }

            this_->Run();
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

void CustomizationSession::Run(bool* modConfigChanged) noexcept {
    if (modConfigChanged) {
        *modConfigChanged = false;
    }

    std::optional<StorageManager::ModConfigChangeNotification>
        modConfigChangeNotification;
    try {
        modConfigChangeNotification.emplace();
    } catch (const std::exception& e) {
        LOG(L"ModConfigChangeNotification constructor failed: %S", e.what());
    }

    while (true) {
        HANDLE waitHandles[] = {m_scopedStaticSessionManagerProcess,
                                modConfigChangeNotification
                                    ? modConfigChangeNotification->GetHandle()
                                    : nullptr};
        DWORD waitHandlesCount = modConfigChangeNotification ? 2 : 1;

        DWORD waitResult = WaitForMultipleObjects(waitHandlesCount, waitHandles,
                                                  FALSE, INFINITE);
        if (waitResult == WAIT_OBJECT_0) {
            break;  // done
        }

        if (waitResult == WAIT_OBJECT_0 + 1) {
            // Wait for a bit before notifying about the change, in case more
            // config changes will follow.
            if (WaitForSingleObject(m_scopedStaticSessionManagerProcess, 200) ==
                WAIT_OBJECT_0) {
                break;  // done
            }

            // Exit on mod config change if modConfigChanged is provided.
            if (modConfigChanged) {
                *modConfigChanged = true;
                break;
            }

            try {
                m_modsManager.ReloadModsAndSettings();
            } catch (const std::exception& e) {
                LOG(L"ReloadModsAndSettings failed: %S", e.what());
            }

            try {
                modConfigChangeNotification->ContinueMonitoring();
            } catch (const std::exception& e) {
                LOG(L"ContinueMonitoring failed: %S", e.what());
                modConfigChangeNotification.reset();
            }

            continue;
        }

        LOG(L"WaitForMultipleObjects returned %u, last error %u", waitResult,
            GetLastError());
        break;
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
