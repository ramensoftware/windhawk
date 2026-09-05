#include "stdafx.h"

#include "all_processes_injector.h"
#include "dll_inject.h"
#include "functions.h"
#include "logger.h"
#include "object_security.h"
#include "path_matching.h"
#include "pe_image.h"
#include "process_lists.h"
#include "session_metadata_store.h"
#include "session_private_namespace.h"
#include "storage_manager.h"
#include "var_init_once.h"

#ifndef STATUS_NO_MORE_ENTRIES
#define STATUS_NO_MORE_ENTRIES ((NTSTATUS)0x8000001AL)
#endif

namespace {

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

// On ARM64, ntdll.dll is an ARM64X image that carries two builds of
// RtlUserThreadStart: a classic ARM64 (native) build and an x64/ARM64EC build.
// A thread that hasn't started running yet is parked at RtlUserThreadStart, and
// which build's address it reports depends on whether the target process is
// native ARM64 or emulated x64, so both are resolved.

// Leading fields of ntdll's ARM64EC (CHPE) metadata, enough to reach the
// redirection table. Mirrors IMAGE_ARM64EC_METADATA from the kernel-mode
// ntimage.h header, which isn't included here.
struct Arm64ecMetadata {
    ULONG Version;
    ULONG CodeMap;
    ULONG CodeMapCount;
    ULONG CodeRangesToEntryPoints;
    ULONG RedirectionMetadata;  // RVA of an Arm64ecRedirectionEntry array
    ULONG Dispatch[5];
    ULONG AlternateEntryPoint;
    ULONG AuxiliaryIAT;
    ULONG CodeRangesToEntryPointsCount;
    ULONG RedirectionMetadataCount;
};

// ARM64EC metadata versions that begin with the fields declared above. A
// version outside this range may lay them out differently, so its metadata is
// ignored rather than misread.
constexpr ULONG kArm64ecMetadataMinVersion = 1;
constexpr ULONG kArm64ecMetadataMaxVersion = 2;

// Pairs an ARM64EC fast-forward stub with its function body, both as RVAs.
struct Arm64ecRedirectionEntry {
    ULONG Source;       // the fast-forward stub
    ULONG Destination;  // the function body
};

// Resolves the x64/ARM64EC RtlUserThreadStart body, where an emulated x64
// thread begins. GetProcAddress in this emulated x64 process returns an ARM64EC
// fast-forward stub rather than the body, so the CHPE redirection table is used
// to map the stub to the real body.
void* GetEmulatedX64RtlUserThreadStart(HMODULE hNtdll) {
    auto* base = reinterpret_cast<BYTE*>(hNtdll);

    void* stub = GetProcAddress(hNtdll, "RtlUserThreadStart");
    THROW_LAST_ERROR_IF_NULL(stub);

    // Every check below falls back to the stub, which is a usable answer for a
    // build that exports the body directly and the only sane one for an image
    // that doesn't parse.
    auto image = Functions::PeImage::FromBase(base);
    // The load config is read through the 64-bit layout.
    if (!image || !image->is64Bit()) {
        return stub;
    }

    const IMAGE_DATA_DIRECTORY* loadConfigDir =
        image->DataDirectory(IMAGE_DIRECTORY_ENTRY_LOAD_CONFIG);

    constexpr DWORD kLoadConfigMinSize =
        offsetof(IMAGE_LOAD_CONFIG_DIRECTORY64, CHPEMetadataPointer) +
        sizeof(IMAGE_LOAD_CONFIG_DIRECTORY64::CHPEMetadataPointer);

    if (!loadConfigDir || !loadConfigDir->VirtualAddress ||
        loadConfigDir->Size < kLoadConfigMinSize) {
        return stub;
    }

    auto* loadConfig = static_cast<const IMAGE_LOAD_CONFIG_DIRECTORY64*>(
        image->At(loadConfigDir->VirtualAddress, kLoadConfigMinSize));
    if (!loadConfig) {
        return stub;
    }

    // CHPEMetadataPointer holds a VA, already relocated in the loaded image.
    std::optional<ULONG> metadataRva =
        image->RvaFromVa(loadConfig->CHPEMetadataPointer);
    if (!metadataRva) {
        return stub;
    }

    auto* metadata =
        Functions::PeImageAt<Arm64ecMetadata>(*image, *metadataRva);
    if (!metadata || metadata->Version < kArm64ecMetadataMinVersion ||
        metadata->Version > kArm64ecMetadataMaxVersion) {
        return stub;
    }

    if (!metadata->RedirectionMetadata) {
        return stub;
    }

    auto redirection = Functions::PeImageArray<Arm64ecRedirectionEntry>(
        *image, metadata->RedirectionMetadata,
        metadata->RedirectionMetadataCount);

    std::optional<ULONG> stubRva =
        image->RvaFromVa(reinterpret_cast<ULONGLONG>(stub));
    if (!stubRva) {
        return stub;
    }

    for (const auto& entry : redirection) {
        // The table's last entry can carry a null destination, which names no
        // body to redirect to.
        if (entry.Source == *stubRva && entry.Destination &&
            entry.Destination < image->imageSize()) {
            return base + entry.Destination;
        }
    }

    return stub;
}

// A PE mapped as a flat view of the file's bytes, where an RVA goes through the
// section table rather than being an offset from the base. The image on disk is
// untrusted input as far as this parser is concerned, so every read goes
// through a bounds check: reading past the end of a mapped view raises an
// in-page error, and a malformed image would otherwise have the headers
// reinterpreted as export tables.
struct FilePeImage {
    const BYTE* base;
    size_t fileSize;
    const IMAGE_SECTION_HEADER* sections;
    WORD sectionCount;

    const BYTE* FileAt(size_t offset, size_t size) const {
        if (offset > fileSize || size > fileSize - offset) {
            return nullptr;
        }

        return base + offset;
    }

    // Null if the span isn't fully backed by a section's raw data.
    const void* At(ULONG rva, size_t size) const {
        for (WORD i = 0; i < sectionCount; i++) {
            const auto& section = sections[i];
            if (rva < section.VirtualAddress ||
                rva - section.VirtualAddress >= section.Misc.VirtualSize) {
                continue;
            }

            DWORD delta = rva - section.VirtualAddress;
            if (delta >= section.SizeOfRawData ||
                size > section.SizeOfRawData - delta) {
                return nullptr;
            }

            return FileAt(static_cast<size_t>(section.PointerToRawData) + delta,
                          size);
        }

        return nullptr;
    }

    // Strings are of unknown length, so they're bounded by what follows them in
    // the file instead of by a size known up front.
    const char* String(ULONG rva) const {
        auto* ptr = static_cast<const BYTE*>(At(rva, 1));
        if (!ptr) {
            return nullptr;
        }

        auto* str = reinterpret_cast<const char*>(ptr);
        size_t available = fileSize - (ptr - base);
        return strnlen(str, available) < available ? str : nullptr;
    }
};

// Resolves the classic ARM64 (native) RtlUserThreadStart, where a native ARM64
// thread begins. The ARM64X relocations that rewrite ntdll's export table to
// the ARM64EC view are applied only in memory, so the native export RVA is read
// from the on-disk image and rebased onto the loaded module.
void* GetNativeArm64RtlUserThreadStart(HMODULE hNtdll) {
    std::wstring ntdllPath = wil::GetModuleFileName<std::wstring>(hNtdll);

    wil::unique_hfile file(CreateFile(ntdllPath.c_str(), GENERIC_READ,
                                      FILE_SHARE_READ, nullptr, OPEN_EXISTING,
                                      0, nullptr));
    THROW_LAST_ERROR_IF(!file);

    LARGE_INTEGER fileSizeLarge;
    THROW_IF_WIN32_BOOL_FALSE(GetFileSizeEx(file.get(), &fileSizeLarge));
    auto fileSize = static_cast<size_t>(fileSizeLarge.QuadPart);

    wil::unique_handle mapping(
        CreateFileMapping(file.get(), nullptr, PAGE_READONLY, 0, 0, nullptr));
    THROW_LAST_ERROR_IF_NULL(mapping);

    wil::unique_mapview_ptr<BYTE> view(reinterpret_cast<BYTE*>(
        MapViewOfFile(mapping.get(), FILE_MAP_READ, 0, 0, 0)));
    THROW_LAST_ERROR_IF(!view);

    FilePeImage image{
        .base = view.get(),
        .fileSize = fileSize,
    };

    auto* dosHeader = reinterpret_cast<const IMAGE_DOS_HEADER*>(
        image.FileAt(0, sizeof(IMAGE_DOS_HEADER)));
    THROW_HR_IF(E_UNEXPECTED, !dosHeader ||
                                  dosHeader->e_magic != IMAGE_DOS_SIGNATURE ||
                                  dosHeader->e_lfanew < 0);

    auto* ntHeaders = reinterpret_cast<const IMAGE_NT_HEADERS64*>(
        image.FileAt(dosHeader->e_lfanew, sizeof(IMAGE_NT_HEADERS64)));
    THROW_HR_IF(E_UNEXPECTED, !ntHeaders ||
                                  ntHeaders->Signature != IMAGE_NT_SIGNATURE ||
                                  ntHeaders->OptionalHeader.Magic !=
                                      IMAGE_NT_OPTIONAL_HDR64_MAGIC);

    const auto& optionalHeader = ntHeaders->OptionalHeader;

    image.sectionCount = ntHeaders->FileHeader.NumberOfSections;
    image.sections = reinterpret_cast<const IMAGE_SECTION_HEADER*>(image.FileAt(
        dosHeader->e_lfanew + offsetof(IMAGE_NT_HEADERS64, OptionalHeader) +
            ntHeaders->FileHeader.SizeOfOptionalHeader,
        image.sectionCount * sizeof(IMAGE_SECTION_HEADER)));
    THROW_HR_IF_NULL(E_UNEXPECTED, image.sections);

    THROW_HR_IF(E_UNEXPECTED, optionalHeader.NumberOfRvaAndSizes <=
                                  IMAGE_DIRECTORY_ENTRY_EXPORT);

    void* result = nullptr;
    Functions::ForEachExportName(
        image, optionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_EXPORT],
        [&](const IMAGE_EXPORT_DIRECTORY& exports, ULONG index,
            std::string_view name) {
            if (name != "RtlUserThreadStart") {
                return false;
            }

            auto nameOrdinals = Functions::PeImageArray<WORD>(
                image, exports.AddressOfNameOrdinals, exports.NumberOfNames);
            auto functionRvas = Functions::PeImageArray<DWORD>(
                image, exports.AddressOfFunctions, exports.NumberOfFunctions);
            THROW_HR_IF(E_UNEXPECTED, index >= nameOrdinals.size());

            WORD ordinal = nameOrdinals[index];
            THROW_HR_IF(E_UNEXPECTED, ordinal >= functionRvas.size());

            DWORD functionRva = functionRvas[ordinal];
            THROW_HR_IF(
                E_UNEXPECTED,
                !functionRva || functionRva >= optionalHeader.SizeOfImage);

            result = reinterpret_cast<BYTE*>(hNtdll) + functionRva;
            return true;
        });

    // A malformed image is one that doesn't name the export, and there's no
    // fallback for this one.
    THROW_HR_IF_NULL(E_UNEXPECTED, result);
    return result;
}

HANDLE CreateProcessInitAPCMutex(DWORD processId, BOOL initialOwner) {
    WCHAR szMutexName[SessionPrivateNamespace::kPrivateNamespaceMaxLen +
                      sizeof("\\ProcessInitAPCMutex-pid=1234567890")];
    int mutexNamePos =
        SessionPrivateNamespace::MakeName(szMutexName, GetCurrentProcessId());
    swprintf_s(szMutexName + mutexNamePos,
               ARRAYSIZE(szMutexName) - mutexNamePos,
               L"\\ProcessInitAPCMutex-pid=%u", processId);

    // The mutex is only waited on and released across the trust boundary, so
    // grant just those rights, not full control, to the sandboxed target
    // processes that open it by name. CreateMutexEx requests the same reduced
    // access.
    constexpr ACCESS_MASK kMutexAccess = SYNCHRONIZE | MUTEX_MODIFY_STATE;

    wil::unique_hlocal secDesc;
    THROW_IF_WIN32_BOOL_FALSE(Functions::BuildSharedObjectSecurityDescriptor(
        kMutexAccess, &secDesc, nullptr));

    SECURITY_ATTRIBUTES secAttr = {sizeof(SECURITY_ATTRIBUTES)};
    secAttr.lpSecurityDescriptor = secDesc.get();
    secAttr.bInheritHandle = FALSE;

    wil::unique_mutex_nothrow mutex(CreateMutexEx(
        &secAttr, szMutexName, initialOwner ? CREATE_MUTEX_INITIAL_OWNER : 0,
        kMutexAccess));
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

#ifdef _M_X64
    if (GetNativeMachine() == IMAGE_FILE_MACHINE_ARM64) {
        m_pRtlUserThreadStart = GetEmulatedX64RtlUserThreadStart(hNtdll);
        m_pRtlUserThreadStartArm64 = GetNativeArm64RtlUserThreadStart(hNtdll);
    } else {
        m_pRtlUserThreadStart = GetProcAddress(hNtdll, "RtlUserThreadStart");
        THROW_LAST_ERROR_IF_NULL(m_pRtlUserThreadStart);
    }
#else
#error "Unsupported architecture"
#endif  // _M_X64

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

void AllProcessesInjector::InjectIntoNewProcesses() noexcept {
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

        try {
            if (ShouldSkipNewProcess(processImageName)) {
                VERBOSE(L"Skipping excluded process %u", dwNewProcessId);
                continue;
            }

            if (Functions::IsProcessBlockingNonMicrosoftBinaries(hNewProcess)) {
                VERBOSE(
                    L"Skipping process %u, it only allows Microsoft-signed "
                    L"images",
                    dwNewProcessId);
                continue;
            }

            InjectIntoNewProcess(hNewProcess, dwNewProcessId,
                                 ShouldAttachExemptThread(processImageName));
            count++;
        } catch (const wil::ResultException& e) {
            switch (e.GetErrorCode()) {
                // STATUS_PROCESS_IS_TERMINATING
                case HRESULT_FROM_NT(0xC000010A):
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

    SweepDeadSessionMetadataThrottled(count);
}

void AllProcessesInjector::SweepDeadSessionMetadataThrottled(
    int newProcessesInjected) {
    m_processesSinceLastSweep += newProcessesInjected;

    // This sweep only reclaims entries left by processes that exit while no
    // dialog is watching: the app prunes entries as it reads a category and the
    // whole subtree is deleted on session end, so exit-driven cleanup is mostly
    // delegated to those paths. It's therefore a background hygiene pass, not
    // time-critical - run it at most once an interval, and only after enough
    // new processes have been injected to suggest meaningful registry churn.
    constexpr ULONGLONG kSweepIntervalMs = 60000;  // 1 minute
    constexpr int kMinProcessesBetweenSweeps = 100;

    // Marks the last time the throttle gate was evaluated, whether or not a
    // sweep followed, so the interval check below can't fire more than once per
    // interval regardless of injection volume.
    ULONGLONG now = GetTickCount64();
    if (m_lastSweepCheckTick != 0 &&
        now - m_lastSweepCheckTick < kSweepIntervalMs) {
        return;
    }
    m_lastSweepCheckTick = now;

    if (m_processesSinceLastSweep > kMinProcessesBetweenSweeps) {
        m_processesSinceLastSweep = 0;
        m_sessionMetadataStore.SweepDeadEntries();
    }
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

#ifdef _M_X64
        CONTEXT c;
        c.ContextFlags = CONTEXT_CONTROL;
        THROW_IF_WIN32_BOOL_FALSE(GetThreadContext(suspendedThread.get(), &c));

        switch (GetNativeMachine()) {
            case IMAGE_FILE_MACHINE_AMD64:
                if (c.Rip == (DWORD64)m_pRtlUserThreadStart) {
                    threadNotStartedYet = true;
                }
                break;

            case IMAGE_FILE_MACHINE_ARM64:
                if (c.Rip == (DWORD64)m_pRtlUserThreadStart ||
                    c.Rip == (DWORD64)m_pRtlUserThreadStartArm64) {
                    threadNotStartedYet = true;
                }
                break;

            default:
                throw std::runtime_error("Unsupported architecture");
        }
#else
#error "Unsupported architecture"
#endif  // _M_X64

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
