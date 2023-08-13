#pragma once

namespace SessionPrivateNamespace {

constexpr size_t kPrivateNamespaceMaxLen =
    sizeof("WindhawkSession1234567890") - 1;

int MakeName(WCHAR szPrivateNamespaceName[kPrivateNamespaceMaxLen + 1],
             DWORD dwSessionManagerProcessId) noexcept;
wil::unique_private_namespace_destroy Create(DWORD dwSessionManagerProcessId);
wil::unique_private_namespace_close Open(DWORD dwSessionManagerProcessId);

}  // namespace SessionPrivateNamespace
