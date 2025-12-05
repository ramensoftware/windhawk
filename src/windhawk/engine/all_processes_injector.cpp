#include "stdafx.h"

#include "all_processes_injector.h"
#include "dll_inject.h"
#include "functions.h"
#include "logger.h"
#include "process_lists.h"
#include "session_private_namespace.h"
#include "storage_manager.h"
#include "var_init_once.h"

#ifndef STATUS_NO_MORE_ENTRIES
#define STATUS_NO_MORE_ENTRIES ((NTSTATUS)0x8000001AL)
#endif

namespace {

struct __declspec(align(16)) MY_CONTEXT_AMD64 {
    DWORD64 dummy1[6];
    DWORD ContextFlags;
    DWORD MxCsr;
    WORD SegCs;
    WORD SegDs;
    WORD SegEs;
    WORD SegFs;
    WORD SegGs;
    WORD SegSs;
    DWORD EFlags;
    DWORD64 dummy2[6];
    DWORD64 Rax;
    DWORD64 Rcx;
    DWORD64 Rdx;
    DWORD64 Rbx;
    DWORD64 Rsp;
    DWORD64 Rbp;
    DWORD64 Rsi;
    DWORD64 Rdi;
    DWORD64 R8;
    DWORD64 R9;
    DWORD64 R10;
    DWORD64 R11;
    DWORD64 R12;
    DWORD64 R13;
    DWORD64 R14;
    DWORD64 R15;
    DWORD64 Rip;
    DWORD64 dummy3[122];
};

#define MY_CONTEXT_AMD64_CONTROL 0x100001

USHORT GetNativeMachineImpl() {
    using IsWow64Process2_t = BOOL(WINAPI*)(
        HANDLE hProcess, USHORT * pProcessMachine, USHORT * pNativeMachine);

    IsWow64Process2_t pIsWow64Process2 = nullptr;
    HMODULE kernel32Module = GetModuleHandle(L"kernel32.dll");
    if (kernel32Module) {
        pIsWow64Process2 = reinterpret_cast<IsWow64Process2_t>(
            GetProcAddress(kernel32Module, "IsWow64Process2"));
    }

    if (pIsWow64Process2) {
        USHORT processMachine = 0;
        USHORT nativeMachine = 0;
        if (pIsWow64Process2(GetCurrentProcess(), &processMachine,
                             &nativeMachine)) {
            return nativeMachine;
        }

        return IMAGE_FILE_MACHINE_UNKNOWN;
    }

#if defined(_M_IX86)
    BOOL isWow64Process = FALSE;
    if (IsWow64Process(GetCurrentProcess(), &isWow64Process)) {
        return isWow64Process ? IMAGE_FILE_MACHINE_AMD64
                              : IMAGE_FILE_MACHINE_I386;
    }
#elif defined(_M_X64)
    return IMAGE_FILE_MACHINE_AMD64;
#else
    // ARM64 OSes should have IsWow64Process2. Other architectures aren't
    // supported.
#endif

    return IMAGE_FILE_MACHINE_UNKNOWN;
}

USHORT GetNativeMachine() {
    STATIC_INIT_ONCE_TRIVIAL(USHORT, nativeMachine, GetNativeMachineImpl());
    return nativeMachine;
}

// This function is used to get the address of the x64 stub of
// RtlUserThreadStart on ARM64. It's done by creating a suspended process and
// querying its initial instruction pointer. For details of why it's needed,
// look for the mention of RtlUserThreadStart in
// https://m417z.com/Implementing-Global-Injection-and-Hooking-in-Windows/.
DWORD64 GetRtlUserThreadStart_x64OnArm64() {
    std::filesystem::path x64HelperPath =
        wil::GetModuleFileName<std::wstring>();
    x64HelperPath.replace_filename(L"windhawk-x64-helper.exe");

    STARTUPINFO si = {sizeof(STARTUPINFO)};
    wil::unique_process_information process;

    THROW_IF_WIN32_BOOL_FALSE(
        CreateProcess(x64HelperPath.c_str(), nullptr, nullptr, nullptr, FALSE,
                      NORMAL_PRIORITY_CLASS | CREATE_SUSPENDED, nullptr,
                      nullptr, &si, &process));

    auto terminateProcessOnScopeExit =
        wil::scope_exit([&process] { TerminateProcess(process.hProcess, 0); });

#ifdef _M_IX86
    auto ntdll = wow64pp::module_handle("ntdll.dll");
    auto pNtGetContextThread = wow64pp::import(ntdll, "NtGetContextThread");

    ARM64_NT_CONTEXT context;
    auto result64 = wow64pp::call_function(
        pNtGetContextThread, wow64pp::handle_to_uint64(process.hThread),
        wow64pp::ptr_to_uint64(&context));
    NTSTATUS result = static_cast<NTSTATUS>(result64);
    THROW_IF_NTSTATUS_FAILED(result);

    return context.Pc;
#else
#error "Unsupported architecture"
#endif  // _M_IX86
}

void GetThreadContext64(HANDLE thread, CONTEXT* context) {
    STATIC_INIT_ONCE_TRIVIAL(DWORD64, pNtGetContextThread, []() {
        auto ntdll = wow64pp::module_handle("ntdll.dll");
        return wow64pp::import(ntdll, "NtGetContextThread");
    }());

    auto result64 = wow64pp::call_function(pNtGetContextThread,
                                           wow64pp::handle_to_uint64(thread),
                                           wow64pp::ptr_to_uint64(context));
    NTSTATUS result = static_cast<NTSTATUS>(result64);
    THROW_IF_NTSTATUS_FAILED(result);
}

HANDLE CreateProcessInitAPCMutex(DWORD processId, BOOL initialOwner) {
    WCHAR szMutexName[SessionPrivateNamespace::kPrivateNamespaceMaxLen +
                      sizeof("\\ProcessInitAPCMutex-pid=1234567890")];
    int mutexNamePos =
        SessionPrivateNamespace::MakeName(szMutexName, GetCurrentProcessId());
    swprintf_s(szMutexName + mutexNamePos,
               ARRAYSIZE(szMutexName) - mutexNamePos,
               L"\\ProcessInitAPCMutex-pid=%u", processId);

    wil::unique_hlocal secDesc;
    THROW_IF_WIN32_BOOL_FALSE(
        Functions::GetFullAccessSecurityDescriptor(&secDesc, nullptr));

    SECURITY_ATTRIBUTES secAttr = {sizeof(SECURITY_ATTRIBUTES)};
    secAttr.lpSecurityDescriptor = secDesc.get();
    secAttr.bInheritHandle = FALSE;

    wil::unique_mutex_nothrow mutex(
        CreateMutex(&secAttr, initialOwner, szMutexName));
    THROW_LAST_ERROR_IF_NULL(mutex);

    return mutex.release();
}

HANDLE OpenProcessInitAPCMutex(DWORD processId, DWORD desiredAccess) {
    WCHAR szMutexName[SessionPrivateNamespace::kPrivateNamespaceMaxLen +
                      sizeof("\\ProcessInitAPCMutex-pid=1234567890")];
    int mutexNamePos =
        SessionPrivateNamespace::MakeName(szMutexName, GetCurrentProcessId());
    swprintf_s(szMutexName + mutexNamePos,
               ARRAYSIZE(szMutexName) - mutexNamePos,
               L"\\ProcessInitAPCMutex-pid=%u", processId);

    return OpenMutex(desiredAccess, FALSE, szMutexName);
}

}  // namespace

AllProcessesInjector::AllProcessesInjector() {
    HMODULE hNtdll = GetModuleHandle(L"ntdll.dll");
    THROW_LAST_ERROR_IF_NULL(hNtdll);

    m_NtGetNextProcess =
        (NtGetNextProcess_t)GetProcAddress(hNtdll, "NtGetNextProcess");
    THROW_LAST_ERROR_IF_NULL(m_NtGetNextProcess);

    m_NtGetNextThread =
        (NtGetNextThread_t)GetProcAddress(hNtdll, "NtGetNextThread");
    THROW_LAST_ERROR_IF_NULL(m_NtGetNextThread);

#ifdef _M_IX86
    USHORT nativeMachine = GetNativeMachine();
    if (nativeMachine == IMAGE_FILE_MACHINE_I386) {
        m_pRtlUserThreadStart = wow64pp::ptr_to_uint64(
            GetProcAddress(hNtdll, "RtlUserThreadStart"));
    } else {
        auto ntdll = wow64pp::module_handle("ntdll.dll");
        m_pRtlUserThreadStart = wow64pp::import(ntdll, "RtlUserThreadStart");

        if (nativeMachine == IMAGE_FILE_MACHINE_ARM64) {
            m_pRtlUserThreadStart_x64OnArm64 =
                GetRtlUserThreadStart_x64OnArm64();
        }
    }
#else
#error "Unsupported architecture"
#endif  // _M_IX86
    THROW_LAST_ERROR_IF(m_pRtlUserThreadStart == 0);

    m_appPrivateNamespace =
        SessionPrivateNamespace::Create(GetCurrentProcessId());

    auto settings = StorageManager::GetInstance().GetAppConfig(L"Settings");
    m_includePattern = settings->GetString(L"Include").value_or(L"");
    m_excludePattern = settings->GetString(L"Exclude").value_or(L"");
    m_threadAttachExemptPattern =
        settings->GetString(L"ThreadAttachExempt").value_or(L"");

    if (!settings->GetInt(L"InjectIntoCriticalProcesses").value_or(0)) {
        if (!m_excludePattern.empty()) {
            m_excludePattern += L'|';
        }

        m_excludePattern += ProcessLists::kCriticalProcesses;
    }

    if (!settings->GetInt(L"InjectIntoIncompatiblePrograms").value_or(0)) {
        if (!m_excludePattern.empty()) {
            m_excludePattern += L'|';
        }

        m_excludePattern += ProcessLists::kIncompatiblePrograms;
    }

    if (!settings->GetInt(L"InjectIntoGames").value_or(0)) {
        if (!m_excludePattern.empty()) {
            m_excludePattern += L'|';
        }

        m_excludePattern += ProcessLists::kGames;
    }
}

int AllProcessesInjector::InjectIntoNewProcesses() noexcept {
    int count = 0;

    while (true) {
        // Note: If we don't have the required permissions, the process is
        // skipped.
        HANDLE hNewProcess;
        NTSTATUS status = m_NtGetNextProcess(
            m_lastEnumeratedProcess.get(),
            SYNCHRONIZE | DllInject::kProcessAccess, 0, 0, &hNewProcess);
        if (!SUCCEEDED_NTSTATUS(status)) {
            if (status != STATUS_NO_MORE_ENTRIES) {
                LOG(L"NtGetNextProcess error: %08X", status);
            }

            break;
        }

        m_lastEnumeratedProcess.reset(hNewProcess);

        if (WaitForSingleObject(hNewProcess, 0) == WAIT_OBJECT_0) {
            // Process is no longer alive.
            continue;
        }

        DWORD dwNewProcessId = GetProcessId(hNewProcess);
        if (dwNewProcessId == 0) {
            LOG(L"GetProcessId error: %u", GetLastError());
            continue;
        }

        std::wstring processImageName;
        switch (HRESULT hr = wil::QueryFullProcessImageName<std::wstring>(
                    hNewProcess, 0, processImageName)) {
            case S_OK:
                break;

            case HRESULT_FROM_WIN32(ERROR_ACCESS_DENIED):
                // Often means the process is terminating.
                VERBOSE(L"Process %u is inaccessible (likely terminating)",
                        dwNewProcessId);
                continue;

            // https://stackoverflow.com/a/74456572
            case HRESULT_FROM_WIN32(ERROR_GEN_FAILURE):
                VERBOSE(L"Process %u is likely terminating", dwNewProcessId);
                continue;

            default:
                LOG(L"QueryFullProcessImageName error for process %u: %08X",
                    dwNewProcessId, hr);
                continue;
        }

        if (ShouldSkipNewProcess(processImageName)) {
            VERBOSE(L"Skipping excluded process %u", dwNewProcessId);
            continue;
        }

        try {
            InjectIntoNewProcess(hNewProcess, dwNewProcessId,
                                 ShouldAttachExemptThread(processImageName));
            count++;
        } catch (const wil::ResultException& e) {
            switch (e.GetErrorCode()) {
                // STATUS_PROCESS_IS_TERMINATING
                case 0xC000010A:
                    VERBOSE(L"Process %u is terminating: %S", dwNewProcessId,
                            e.what());
                    break;

                case HRESULT_FROM_WIN32(ERROR_ACCESS_DENIED):
                    // May happen if process is terminating.
                    VERBOSE(L"Access denied for process %u: %S", dwNewProcessId,
                            e.what());
                    break;

                default:
                    LOG(L"Error handling a new process %u: %S", dwNewProcessId,
                        e.what());
                    break;
            }
        } catch (const std::exception& e) {
            LOG(L"Error handling a new process %u: %S", dwNewProcessId,
                e.what());
        }
    }

    return count;
}

bool AllProcessesInjector::ShouldSkipNewProcess(
    std::wstring_view processImageName) const {
    return Functions::DoesPathMatchPattern(processImageName,
                                           m_excludePattern) &&
           !Functions::DoesPathMatchPattern(processImageName, m_includePattern);
}

bool AllProcessesInjector::ShouldAttachExemptThread(
    std::wstring_view processImageName) const {
    return Functions::DoesPathMatchPattern(processImageName,
                                           m_threadAttachExemptPattern);
}

void AllProcessesInjector::InjectIntoNewProcess(HANDLE hProcess,
                                                DWORD dwProcessId,
                                                bool threadAttachExempt) {
    // We check whether the process began running or not. If it didn't, it's
    // supposed to have only one thread which has its instruction pointer at
    // RtlUserThreadStart. For other cases, we assume the main thread was
    // resumed.
    //
    // If the process didn't begin running, creating a remote thread might be
    // too early and unsafe. One known problem with this is with console apps -
    // if we trigger console initialization (KERNELBASE!ConsoleCommitState)
    // before the parent process notified csrss.exe
    // (KERNELBASE!CsrClientCallServer), csrss.exe returns an access denied
    // error and the parent's CreateProcess call fails.
    //
    // If the process is the current process, we skip this check since it
    // obviously began running, and we don't want to suspend the current thread
    // and cause a deadlock.

    wil::unique_process_handle suspendedThread;

    if (dwProcessId != GetCurrentProcessId()) {
        DWORD threadAccess = THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT |
                             DllInject::kApcThreadsAccess;

        wil::unique_process_handle thread1;
        THROW_IF_NTSTATUS_FAILED(
            m_NtGetNextThread(hProcess, nullptr, threadAccess, 0, 0, &thread1));

        wil::unique_process_handle thread2;
        NTSTATUS status = m_NtGetNextThread(hProcess, thread1.get(),
                                            threadAccess, 0, 0, &thread2);
        if (status == STATUS_NO_MORE_ENTRIES) {
            // Exactly one thread.
            DWORD previousSuspendCount = SuspendThread(thread1.get());
            THROW_LAST_ERROR_IF(previousSuspendCount == (DWORD)-1);

            if (previousSuspendCount == 0) {
                // The thread was already running.
                ResumeThread(thread1.get());
            } else {
                suspendedThread = std::move(thread1);
            }
        } else {
            THROW_IF_NTSTATUS_FAILED(status);
        }
    }

    if (suspendedThread) {
        auto suspendThreadCleanup = wil::scope_exit(
            [&suspendedThread] { ResumeThread(suspendedThread.get()); });

        bool threadNotStartedYet = false;

#ifdef _M_IX86
        switch (GetNativeMachine()) {
            case IMAGE_FILE_MACHINE_I386: {
                CONTEXT c;
                c.ContextFlags = CONTEXT_CONTROL;
                THROW_IF_WIN32_BOOL_FALSE(
                    GetThreadContext(suspendedThread.get(), &c));
                if (c.Eip == m_pRtlUserThreadStart) {
                    threadNotStartedYet = true;
                }
                break;
            }

            case IMAGE_FILE_MACHINE_AMD64: {
                MY_CONTEXT_AMD64 c;
                c.ContextFlags = MY_CONTEXT_AMD64_CONTROL;
                GetThreadContext64(suspendedThread.get(), (CONTEXT*)&c);
                if (c.Rip == m_pRtlUserThreadStart) {
                    threadNotStartedYet = true;
                }
                break;
            }

            case IMAGE_FILE_MACHINE_ARM64: {
                ARM64_NT_CONTEXT c;
                c.ContextFlags = CONTEXT_ARM64_CONTROL;
                GetThreadContext64(suspendedThread.get(), (CONTEXT*)&c);
                if (c.Pc == m_pRtlUserThreadStart ||
                    c.Pc == m_pRtlUserThreadStart_x64OnArm64) {
                    threadNotStartedYet = true;
                }
                break;
            }

            default: {
                throw std::runtime_error("Unsupported architecture");
            }
        }
#else
#error "Unsupported architecture"
#endif  // _M_IX86

        if (threadNotStartedYet) {
            wil::unique_mutex_nothrow mutex(
                CreateProcessInitAPCMutex(dwProcessId, TRUE));
            if (GetLastError() == ERROR_ALREADY_EXISTS) {
                return;  // APC was already created
            }

            auto mutexLock = mutex.ReleaseMutex_scope_exit();

            DllInject::DllInject(hProcess, suspendedThread.get(),
                                 GetCurrentProcess(), mutex.get(),
                                 threadAttachExempt);
            VERBOSE(L"DllInject succeeded for new process %u via APC",
                    dwProcessId);

            return;
        }
    }

    wil::unique_mutex_nothrow mutex(
        OpenProcessInitAPCMutex(dwProcessId, SYNCHRONIZE));
    if (mutex) {
        return;  // APC was already created
    }

    DllInject::DllInject(hProcess, nullptr, GetCurrentProcess(), nullptr,
                         threadAttachExempt);
    VERBOSE(L"DllInject succeeded for new process %u via a remote thread",
            dwProcessId);
}
