#pragma once

#include "shared_functions.h"

namespace Functions {

bool wcsmatch(PCWSTR pat, size_t plen, PCWSTR str, size_t slen);
bool DoesPathMatchPattern(std::wstring_view path,
                          std::wstring_view pattern,
                          bool explicitOnly = false);
void** FindImportPtr(HMODULE hFindInModule,
                     PCSTR pModuleName,
                     PCSTR pImportName);
// Builds a self-contained security descriptor that grants `access` to the
// "Everyone" group and the "All [Restricted] App Packages" groups, with a
// protected DACL and an Untrusted integrity label, so a shared kernel object is
// reachable from sandboxed and low-integrity target processes. `access` must be
// object-type-specific rights (not GENERIC_*) and must exclude WRITE_DAC and
// WRITE_OWNER, which would let those processes rewrite the object's ACL or take
// ownership across the trust boundary.
BOOL BuildSharedObjectSecurityDescriptor(
    ACCESS_MASK access,
    _Outptr_ PSECURITY_DESCRIPTOR* SecurityDescriptor,
    _Out_opt_ PULONG SecurityDescriptorSize);

struct DaclAce {
    PSID sid;
    ACCESS_MASK access;  // generic or specific rights
    DWORD inheritFlags;  // e.g. CONTAINER_INHERIT_ACE, OBJECT_INHERIT_ACE
};

// Idempotently ensures the file or directory has an explicit allow ACE for each
// entry, added to the existing DACL with inheritance left intact. Makes no
// change when the ACEs are already present. Returns a Win32 error code
// (ERROR_SUCCESS on success or when nothing needed to change).
DWORD EnsureFileDaclContainsAces(PCWSTR path,
                                 const DaclAce* aces,
                                 size_t aceCount);

// Same as above for a registry key (created if missing) in the 64-bit view.
DWORD EnsureRegistryKeyDaclContainsAces(HKEY hKey,
                                        PCWSTR subKey,
                                        const DaclAce* aces,
                                        size_t aceCount);

// https://waleedassar.blogspot.com/2012/12/skipthreadattach.html
enum MyCreateRemoteThreadFlags : ULONG {
    MY_REMOTE_THREAD_CREATE_SUSPENDED = 0x01,
    MY_REMOTE_THREAD_THREAD_ATTACH_EXEMPT = 0x02,
    MY_REMOTE_THREAD_HIDE_FROM_DEBUGGER = 0x04,
    MY_REMOTE_THREAD_LOADER_WORKER = 0x10,          // since THRESHOLD
    MY_REMOTE_THREAD_SKIP_LOADER_INIT = 0x20,       // since REDSTONE2
    MY_REMOTE_THREAD_BYPASS_PROCESS_FREEZE = 0x40,  // since 19H1
};

// Using MyCreateRemoteThread instead of CreateRemoteThread allows providing
// extra flags. We use the MY_REMOTE_THREAD_THREAD_ATTACH_EXEMPT flag to reduce
// incompatibility with other processes.
HANDLE MyCreateRemoteThread(HANDLE hProcess,
                            LPTHREAD_START_ROUTINE lpStartAddress,
                            LPVOID lpParameter,
                            ULONG createFlags);

bool ModuleGetPDBInfo(HANDLE hOsHandle,
                      _Out_ GUID* pGuidSignature,
                      _Out_ DWORD* pdwAge);
std::string GetModuleVersion(HMODULE hModule);

}  // namespace Functions
