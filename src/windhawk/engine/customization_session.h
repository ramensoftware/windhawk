#pragma once

#include "mods_manager.h"
#include "new_process_injector.h"
#include "no_destructor.h"
#include "storage_manager.h"
#include "var_init_once.h"

class CustomizationSession {
   private:
    // https://devblogs.microsoft.com/oldnewthing/20220721-00/?p=106879
    struct ConstructorSecret {
        explicit ConstructorSecret() = default;
    };

   public:
    CustomizationSession(const CustomizationSession&) = delete;
    CustomizationSession& operator=(const CustomizationSession&) = delete;

    static void Start(bool runningFromAPC,
                      bool threadAttachExempt,
                      wil::unique_process_handle sessionManagerProcess,
                      wil::unique_mutex_nothrow sessionMutex);
    static DWORD GetSessionManagerProcessId();
    static FILETIME GetSessionManagerProcessCreationTime();
    static bool IsEndingSoon();

    // Must be public for std emplace and destruction, but shouldn't be used
    // outside of this file.
    CustomizationSession(ConstructorSecret constructorSecret,
                         bool runningFromAPC,
                         bool threadAttachExempt,
                         wil::unique_process_handle sessionManagerProcess,
                         wil::unique_mutex_nothrow sessionMutex);
    ~CustomizationSession();

   private:
    // Used to hold a single process handle which can be accessed from static
    // functions.
    class ScopedStaticSessionManagerProcess {
       public:
        ScopedStaticSessionManagerProcess(
            const ScopedStaticSessionManagerProcess&) = delete;
        ScopedStaticSessionManagerProcess& operator=(
            const ScopedStaticSessionManagerProcess&) = delete;

        ScopedStaticSessionManagerProcess(wil::unique_process_handle handle) {
            GetInstance().emplace(std::move(handle));
        }
        ~ScopedStaticSessionManagerProcess() { GetInstance().reset(); }
        static std::optional<wil::unique_process_handle>& GetInstance() {
            STATIC_INIT_ONCE(NoDestructorIfTerminating<
                                 std::optional<wil::unique_process_handle>>,
                             handle);
            return **handle;
        }
        operator HANDLE() { return GetInstance().value().get(); }
    };

    class MinHookScopeInit {
       public:
        MinHookScopeInit(const MinHookScopeInit&) = delete;
        MinHookScopeInit& operator=(const MinHookScopeInit&) = delete;

        MinHookScopeInit(MH_THREAD_FREEZE_METHOD freezeMethod);
        ~MinHookScopeInit();
    };

    class MinHookScopeApply {
       public:
        MinHookScopeApply(const MinHookScopeApply&) = delete;
        MinHookScopeApply& operator=(const MinHookScopeApply&) = delete;

        MinHookScopeApply();
        ~MinHookScopeApply();
    };

    class MainLoopRunner {
       public:
        MainLoopRunner() noexcept;

        enum class Result {
            kReloadModsAndSettings,
            kCompleted,
            kError,
        };

        Result Run(HANDLE sessionManagerProcess) noexcept;
        bool ContinueMonitoring() noexcept;
        bool CanRunAcrossThreads() noexcept;

       private:
        std::optional<StorageManager::ModConfigChangeNotification>
            m_modConfigChangeNotification;
    };

    static std::optional<CustomizationSession>& GetInstance();

    void StartInitialized(wil::unique_semaphore semaphore,
                          wil::semaphore_release_scope_exit semaphoreLock,
                          bool runningFromAPC) noexcept;
    void RunMainLoopAndDeleteThisWithThreadRecreate() noexcept;
    void RunMainLoop() noexcept;
    void DeleteThis() noexcept;

    bool m_threadAttachExempt;
    ScopedStaticSessionManagerProcess m_scopedStaticSessionManagerProcess;
    wil::unique_mutex_nothrow m_sessionMutex;
    MinHookScopeInit m_minHookScopeInit;
    ModsManager m_modsManager;
    NewProcessInjector m_newProcessInjector;
    MinHookScopeApply m_minHookScopeApply;

    std::optional<MainLoopRunner> m_mainLoopRunner;

    // Must be released after the singleton object is freed. See the careful
    // usage in DeleteThis.
    wil::unique_semaphore m_sessionSemaphore;
    wil::semaphore_release_scope_exit m_sessionSemaphoreLock;
};
