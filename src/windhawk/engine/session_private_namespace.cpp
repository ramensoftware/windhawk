#include "stdafx.h"

#include "functions.h"
#include "session_private_namespace.h"

namespace {

constexpr auto kBoundaryDescriptorName = L"Windhawk";

wil::unique_boundary_descriptor BuildBoundaryDescriptor() {
    wil::unique_boundary_descriptor boundaryDesc(
        CreateBoundaryDescriptor(kBoundaryDescriptorName, 0));
    THROW_LAST_ERROR_IF_NULL(boundaryDesc);

    {
        wil::unique_sid pSID;
        SID_IDENTIFIER_AUTHORITY SIDWorldAuth = SECURITY_WORLD_SID_AUTHORITY;
        THROW_IF_WIN32_BOOL_FALSE(AllocateAndInitializeSid(
            &SIDWorldAuth, 1, SECURITY_WORLD_RID, 0, 0, 0, 0, 0, 0, 0, &pSID));

        THROW_IF_WIN32_BOOL_FALSE(
            AddSIDToBoundaryDescriptor(boundaryDesc.addressof(), pSID.get()));
    }

    {
        wil::unique_sid pSID;
        SID_IDENTIFIER_AUTHORITY SIDMandatoryLabelAuth =
            SECURITY_MANDATORY_LABEL_AUTHORITY;
        THROW_IF_WIN32_BOOL_FALSE(AllocateAndInitializeSid(
            &SIDMandatoryLabelAuth, 1, SECURITY_MANDATORY_MEDIUM_RID, 0, 0, 0,
            0, 0, 0, 0, &pSID));

        THROW_IF_WIN32_BOOL_FALSE(AddIntegrityLabelToBoundaryDescriptor(
            boundaryDesc.addressof(), pSID.get()));
    }

    return boundaryDesc;
}

}  // namespace

namespace SessionPrivateNamespace {

int MakeName(WCHAR szPrivateNamespaceName[kPrivateNamespaceMaxLen + 1],
             DWORD dwSessionManagerProcessId) noexcept {
    static_assert(kPrivateNamespaceMaxLen + 1 ==
                  sizeof("WindhawkSession1234567890"));
    return swprintf_s(szPrivateNamespaceName, kPrivateNamespaceMaxLen + 1,
                      L"WindhawkSession%u", dwSessionManagerProcessId);
}

wil::unique_private_namespace_destroy Create(DWORD dwSessionManagerProcessId) {
    wil::unique_boundary_descriptor boundaryDesc(BuildBoundaryDescriptor());

    wil::unique_hlocal secDesc;
    THROW_IF_WIN32_BOOL_FALSE(
        Functions::GetFullAccessSecurityDescriptor(&secDesc, nullptr));

    SECURITY_ATTRIBUTES secAttr = {sizeof(SECURITY_ATTRIBUTES)};
    secAttr.lpSecurityDescriptor = secDesc.get();
    secAttr.bInheritHandle = FALSE;

    WCHAR szPrivateNamespaceName[kPrivateNamespaceMaxLen + 1];
    MakeName(szPrivateNamespaceName, dwSessionManagerProcessId);

    wil::unique_private_namespace_destroy privateNamespace(
        CreatePrivateNamespace(&secAttr, (void*)boundaryDesc.get(),
                               szPrivateNamespaceName));
    THROW_LAST_ERROR_IF_NULL(privateNamespace);

    return privateNamespace;
}

wil::unique_private_namespace_close Open(DWORD dwSessionManagerProcessId) {
    wil::unique_boundary_descriptor boundaryDesc(BuildBoundaryDescriptor());

    WCHAR szPrivateNamespaceName[kPrivateNamespaceMaxLen + 1];
    MakeName(szPrivateNamespaceName, dwSessionManagerProcessId);

    wil::unique_private_namespace_close privateNamespace(OpenPrivateNamespace(
        (void*)boundaryDesc.get(), szPrivateNamespaceName));
    THROW_LAST_ERROR_IF_NULL(privateNamespace);

    return privateNamespace;
}

}  // namespace SessionPrivateNamespace
