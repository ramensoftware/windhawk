#pragma once

class NewProcessInjector {
   public:
    NewProcessInjector(HANDLE hSessionManagerProcess);
    ~NewProcessInjector();

    // Disable copying and moving to keep using the same counter variable.
    NewProcessInjector(const NewProcessInjector&) = delete;
    NewProcessInjector(NewProcessInjector&&) noexcept = delete;
    NewProcessInjector& operator=(const NewProcessInjector&) = delete;
    NewProcessInjector& operator=(NewProcessInjector&&) noexcept = delete;

   private:
    using CreateProcessInternalW_t =
        BOOL(WINAPI*)(HANDLE hToken,
                      LPCWSTR lpApplicationName,
                      LPWSTR lpCommandLine,
                      LPSECURITY_ATTRIBUTES lpProcessAttributes,
                      LPSECURITY_ATTRIBUTES lpThreadAttributes,
                      BOOL bInheritHandles,
                      DWORD dwCreationFlags,
                      LPVOID lpEnvironment,
                      LPCWSTR lpCurrentDirectory,
                      LPSTARTUPINFOW lpStartupInfo,
                      LPPROCESS_INFORMATION lpProcessInformation,
                      DWORD_PTR unknown);

    static BOOL WINAPI
    CreateProcessInternalW_Hook(HANDLE hToken,
                                LPCWSTR lpApplicationName,
                                LPWSTR lpCommandLine,
                                LPSECURITY_ATTRIBUTES lpProcessAttributes,
                                LPSECURITY_ATTRIBUTES lpThreadAttributes,
                                BOOL bInheritHandles,
                                DWORD dwCreationFlags,
                                LPVOID lpEnvironment,
                                LPCWSTR lpCurrentDirectory,
                                LPSTARTUPINFOW lpStartupInfo,
                                LPPROCESS_INFORMATION lpProcessInformation,
                                DWORD_PTR unknown);
    void HandleCreatedProcess(LPPROCESS_INFORMATION lpProcessInformation);
    bool ShouldSkipNewProcess(HANDLE hProcess, bool* threadAttachExempt);

    // Limited to a single instance at a time.
    static std::atomic<NewProcessInjector*> m_pThis;

    HANDLE m_sessionManagerProcess;
    CreateProcessInternalW_t m_originalCreateProcessInternalW = nullptr;
    std::atomic<int> m_hookProcCallCounter = 0;
    std::wstring m_includePattern;
    std::wstring m_excludePattern;
    std::wstring m_threadAttachExemptPattern;
};
