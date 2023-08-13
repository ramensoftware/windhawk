#pragma once

namespace Functions {

bool wcsmatch(PCWSTR pat, size_t plen, PCWSTR str, size_t slen);

std::vector<std::wstring> SplitString(std::wstring_view s, WCHAR delim);

bool DoesPathMatchPattern(std::wstring_view path, std::wstring_view pattern);

void** FindImportPtr(HMODULE hFindInModule,
                     PCSTR pModuleName,
                     PCSTR pImportName);

BOOL GetFullAccessSecurityDescriptor(
    _Outptr_ PSECURITY_DESCRIPTOR* SecurityDescriptor,
    _Out_opt_ PULONG SecurityDescriptorSize);

// https://waleedassar.blogspot.com/2012/12/skipthreadattach.html
enum MyCreateRemoteThreadFlags : ULONG {
    MY_REMOTE_THREAD_CREATE_SUSPENDED = 0x01,
    MY_REMOTE_THREAD_THREAD_ATTACH_EXEMPT = 0x02,
    MY_REMOTE_THREAD_HIDE_FROM_DEBUGGER = 0x04,
};

// Using MyCreateRemoteThread instead of CreateRemoteThread provides the
// following benefits:
// * On Windows 7, it allows to create a remote thread in a process running in
//   another session.
// * It allows providing extra flags. We use the
//   MY_REMOTE_THREAD_THREAD_ATTACH_EXEMPT flag to reduce incompatibility with
//   other processes.
HANDLE MyCreateRemoteThread(HANDLE hProcess,
                            LPTHREAD_START_ROUTINE lpStartAddress,
                            LPVOID lpParameter,
                            ULONG createFlags);

}  // namespace Functions
