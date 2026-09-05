#pragma once

namespace SessionPrivateNamespace {

constexpr size_t kPrivateNamespaceMaxLen =
    sizeof("WindhawkSession1234567890") - 1;

// Writes the namespace name and returns its length. Throws if the name doesn't
// fit in the buffer, so the return value is always a length.
int MakeName(WCHAR szPrivateNamespaceName[kPrivateNamespaceMaxLen + 1],
             DWORD dwSessionManagerProcessId);
wil::unique_private_namespace_destroy Create(DWORD dwSessionManagerProcessId);
wil::unique_private_namespace_close Open(DWORD dwSessionManagerProcessId);

}  // namespace SessionPrivateNamespace
