#pragma once

class AllProcessesInjector {
   public:
    AllProcessesInjector();

    void InjectIntoNewProcesses() noexcept;

   private:
    bool ShouldSkipNewProcess(std::wstring_view processImageName) const;
    bool ShouldAttachExemptThread(std::wstring_view processImageName) const;
    void InjectIntoNewProcess(HANDLE hProcess,
                              DWORD dwProcessId,
                              bool threadAttachExempt);
    void SweepDeadSessionMetadataThrottled(int newProcessesInjected);

    using NtGetNextProcess_t = NTSTATUS(NTAPI*)(_In_opt_ HANDLE ProcessHandle,
                                                _In_ ACCESS_MASK DesiredAccess,
                                                _In_ ULONG HandleAttributes,
                                                _In_ ULONG Flags,
                                                _Out_ PHANDLE NewProcessHandle);

    using NtGetNextThread_t = NTSTATUS(NTAPI*)(_In_ HANDLE ProcessHandle,
                                               _In_opt_ HANDLE ThreadHandle,
                                               _In_ ACCESS_MASK DesiredAccess,
                                               _In_ ULONG HandleAttributes,
                                               _In_ ULONG Flags,
                                               _Out_ PHANDLE NewThreadHandle);

    NtGetNextProcess_t m_NtGetNextProcess = nullptr;
    NtGetNextThread_t m_NtGetNextThread = nullptr;
    void* m_pRtlUserThreadStart = nullptr;
    void* m_pRtlUserThreadStartArm64 = nullptr;
    wil::unique_private_namespace_destroy m_appPrivateNamespace;
    std::wstring m_includePattern;
    std::wstring m_excludePattern;
    std::wstring m_threadAttachExemptPattern;
    wil::unique_process_handle m_lastEnumeratedProcess;
    int m_processesSinceLastSweep = 0;
    ULONGLONG m_lastSweepCheckTick = 0;
};
