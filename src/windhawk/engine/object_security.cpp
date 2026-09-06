#include "stdafx.h"

#include "object_security.h"

namespace Functions {

namespace {

using unique_acl_local =
    wil::unique_any<PACL, decltype(&::LocalFree), ::LocalFree>;

constexpr GENERIC_MAPPING kFileGenericMapping = {
    FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_GENERIC_EXECUTE,
    FILE_ALL_ACCESS};

constexpr GENERIC_MAPPING kRegistryGenericMapping = {
    KEY_READ, KEY_WRITE, KEY_EXECUTE, KEY_ALL_ACCESS};

// Returns true if the DACL already contains an explicit (non-inherited) allow
// ACE for the entry's SID that applies to the object itself, carries the
// required inheritance flags, and grants at least the requested access. Both
// masks are mapped from generic to specific rights before comparing, so the
// check holds whether the stored ACE uses generic or specific bits.
bool DaclContainsAce(PACL dacl,
                     const DaclAce& ace,
                     const GENERIC_MAPPING& genericMapping) {
    ACCESS_MASK desired = ace.access;
    MapGenericMask(&desired, const_cast<PGENERIC_MAPPING>(&genericMapping));

    ACL_SIZE_INFORMATION sizeInfo;
    if (!GetAclInformation(dacl, &sizeInfo, sizeof(sizeInfo),
                           AclSizeInformation)) {
        return false;
    }

    for (DWORD i = 0; i < sizeInfo.AceCount; i++) {
        void* aceEntry = nullptr;
        if (!GetAce(dacl, i, &aceEntry)) {
            continue;
        }

        auto* header = static_cast<ACE_HEADER*>(aceEntry);
        if (header->AceType != ACCESS_ALLOWED_ACE_TYPE) {
            continue;
        }

        // Only an explicit ACE that applies to the object itself and propagates
        // the way the installer's grant does can satisfy the requirement.
        if (header->AceFlags &
            (INHERITED_ACE | INHERIT_ONLY_ACE | NO_PROPAGATE_INHERIT_ACE)) {
            continue;
        }

        if ((header->AceFlags & ace.inheritFlags) != ace.inheritFlags) {
            continue;
        }

        auto* allowedAce = static_cast<ACCESS_ALLOWED_ACE*>(aceEntry);
        if (!EqualSid(&allowedAce->SidStart, ace.sid)) {
            continue;
        }

        ACCESS_MASK granted = allowedAce->Mask;
        MapGenericMask(&granted, const_cast<PGENERIC_MAPPING>(&genericMapping));
        if ((granted & desired) == desired) {
            return true;
        }
    }

    return false;
}

// Merges any missing ACEs into currentDacl. Sets *changed to false and leaves
// *mergedDacl empty when every ACE is already present (so the caller writes
// nothing). Returns a Win32 error code.
DWORD BuildMergedDacl(PACL currentDacl,
                      const GENERIC_MAPPING& genericMapping,
                      const DaclAce* aces,
                      size_t aceCount,
                      unique_acl_local& mergedDacl,
                      bool* changed) {
    *changed = false;

    // A null DACL grants everyone full access. Rebuilding from it would drop
    // that and lock out SYSTEM and Administrators, so leave it untouched.
    if (!currentDacl) {
        return ERROR_SUCCESS;
    }

    std::vector<EXPLICIT_ACCESS_W> explicitAccess;
    for (size_t i = 0; i < aceCount; i++) {
        if (DaclContainsAce(currentDacl, aces[i], genericMapping)) {
            continue;
        }

        EXPLICIT_ACCESS_W entry = {};
        entry.grfAccessPermissions = aces[i].access;
        entry.grfAccessMode = GRANT_ACCESS;
        entry.grfInheritance = aces[i].inheritFlags;
        entry.Trustee.TrusteeForm = TRUSTEE_IS_SID;
        entry.Trustee.TrusteeType = TRUSTEE_IS_GROUP;
        entry.Trustee.ptstrName = static_cast<LPWSTR>(aces[i].sid);
        explicitAccess.push_back(entry);
    }

    if (explicitAccess.empty()) {
        return ERROR_SUCCESS;
    }

    PACL merged = nullptr;
    DWORD error = SetEntriesInAcl(static_cast<ULONG>(explicitAccess.size()),
                                  explicitAccess.data(), currentDacl, &merged);
    if (error != ERROR_SUCCESS) {
        return error;
    }

    mergedDacl = unique_acl_local{merged};
    *changed = true;
    return ERROR_SUCCESS;
}

// Returns true if the SACL carries a mandatory label ACE of any kind.
bool SaclContainsAnyLabel(PACL sacl) {
    if (!sacl) {
        return false;
    }

    ACL_SIZE_INFORMATION sizeInfo;
    if (!GetAclInformation(sacl, &sizeInfo, sizeof(sizeInfo),
                           AclSizeInformation)) {
        return false;
    }

    for (DWORD i = 0; i < sizeInfo.AceCount; i++) {
        void* aceEntry = nullptr;
        if (!GetAce(sacl, i, &aceEntry)) {
            continue;
        }

        if (static_cast<ACE_HEADER*>(aceEntry)->AceType ==
            SYSTEM_MANDATORY_LABEL_ACE_TYPE) {
            return true;
        }
    }

    return false;
}

}  // namespace

BOOL BuildSharedObjectSecurityDescriptor(
    ACCESS_MASK access,
    _Outptr_ PSECURITY_DESCRIPTOR* SecurityDescriptor,
    _Out_opt_ PULONG SecurityDescriptorSize) {
    // http://rsdn.org/forum/winapi/7510772.flat
    //
    // Grant `access` to the "Everyone" group and to the "All [Restricted] App
    // Packages" groups, so a shared kernel object stays reachable from
    // sandboxed target processes. `access` carries object-type-specific rights
    // only (no GENERIC_*, no WRITE_DAC/WRITE_OWNER): the grantees cross a trust
    // boundary and must not be able to rewrite the object's ACL or take
    // ownership. The integrity label is Untrusted (lowest level) so the object
    // stays reachable from any integrity level.
    //
    // D - DACL
    // P - Protected
    // A - Access Allowed
    // WD - 'All' Group (World)
    // S-1-15-2-1 - All Application Packages
    // S-1-15-2-2 - All Restricted Application Packages
    //
    // S - SACL
    // ML - Mandatory Label
    // NW - No Write-Up policy
    // S-1-16-0 - Untrusted Mandatory Level
    WCHAR stringSecurityDescriptor[256];
    swprintf_s(stringSecurityDescriptor, ARRAYSIZE(stringSecurityDescriptor),
               L"D:P(A;;0x%08lX;;;WD)(A;;0x%08lX;;;S-1-15-2-1)"
               L"(A;;0x%08lX;;;S-1-15-2-2)S:(ML;;NW;;;S-1-16-0)",
               access, access, access);

    return ConvertStringSecurityDescriptorToSecurityDescriptor(
        stringSecurityDescriptor, SDDL_REVISION_1, SecurityDescriptor,
        SecurityDescriptorSize);
}

DWORD EnsureFileDaclContainsAces(PCWSTR path,
                                 const DaclAce* aces,
                                 size_t aceCount) {
    PACL dacl = nullptr;
    PSECURITY_DESCRIPTOR securityDescriptor = nullptr;
    DWORD error = GetNamedSecurityInfo(
        path, SE_FILE_OBJECT, DACL_SECURITY_INFORMATION, nullptr, nullptr,
        &dacl, nullptr, &securityDescriptor);
    if (error != ERROR_SUCCESS) {
        return error;
    }
    wil::unique_hlocal_security_descriptor securityDescriptorOwner(
        securityDescriptor);

    unique_acl_local mergedDacl;
    bool changed = false;
    error = BuildMergedDacl(dacl, kFileGenericMapping, aces, aceCount,
                            mergedDacl, &changed);
    if (error != ERROR_SUCCESS || !changed) {
        return error;
    }

    // SetNamedSecurityInfo takes a mutable object name.
    std::wstring mutablePath(path);
    return SetNamedSecurityInfo(mutablePath.data(), SE_FILE_OBJECT,
                                DACL_SECURITY_INFORMATION, nullptr, nullptr,
                                mergedDacl.get(), nullptr);
}

DWORD EnsureRegistryKeyDaclContainsAces(HKEY hKey,
                                        PCWSTR subKey,
                                        const DaclAce* aces,
                                        size_t aceCount) {
    // Create or open the key in the 64-bit view and operate on the handle, so
    // the ACL is read and written on the same key regardless of the caller's
    // bitness.
    wil::unique_hkey key;
    LSTATUS status = RegCreateKeyEx(
        hKey, subKey, 0, nullptr, REG_OPTION_NON_VOLATILE,
        KEY_WOW64_64KEY | READ_CONTROL | WRITE_DAC, nullptr, &key, nullptr);
    if (status != ERROR_SUCCESS) {
        return status;
    }

    PACL dacl = nullptr;
    PSECURITY_DESCRIPTOR securityDescriptor = nullptr;
    DWORD error =
        GetSecurityInfo(key.get(), SE_REGISTRY_KEY, DACL_SECURITY_INFORMATION,
                        nullptr, nullptr, &dacl, nullptr, &securityDescriptor);
    if (error != ERROR_SUCCESS) {
        return error;
    }
    wil::unique_hlocal_security_descriptor securityDescriptorOwner(
        securityDescriptor);

    unique_acl_local mergedDacl;
    bool changed = false;
    error = BuildMergedDacl(dacl, kRegistryGenericMapping, aces, aceCount,
                            mergedDacl, &changed);
    if (error != ERROR_SUCCESS || !changed) {
        return error;
    }

    return SetSecurityInfo(key.get(), SE_REGISTRY_KEY,
                           DACL_SECURITY_INFORMATION, nullptr, nullptr,
                           mergedDacl.get(), nullptr);
}

DWORD EnsureFileHasNoMandatoryLabel(PCWSTR path) {
    PACL sacl = nullptr;
    PSECURITY_DESCRIPTOR securityDescriptor = nullptr;
    DWORD error = GetNamedSecurityInfo(
        path, SE_FILE_OBJECT, LABEL_SECURITY_INFORMATION, nullptr, nullptr,
        nullptr, &sacl, &securityDescriptor);
    if (error != ERROR_SUCCESS) {
        return error;
    }
    wil::unique_hlocal_security_descriptor securityDescriptorOwner(
        securityDescriptor);

    if (!SaclContainsAnyLabel(sacl)) {
        return ERROR_SUCCESS;
    }

    // An empty SACL is what drops the label; a null one would leave it.
    alignas(DWORD) BYTE aclBuffer[sizeof(ACL)];
    PACL emptyAcl = reinterpret_cast<PACL>(aclBuffer);
    if (!InitializeAcl(emptyAcl, sizeof(aclBuffer), ACL_REVISION)) {
        return GetLastError();
    }

    // SetNamedSecurityInfo takes a mutable object name.
    std::wstring mutablePath(path);
    return SetNamedSecurityInfo(mutablePath.data(), SE_FILE_OBJECT,
                                LABEL_SECURITY_INFORMATION, nullptr, nullptr,
                                nullptr, emptyAcl);
}

DWORD EnsureRegistryKeyHasNoMandatoryLabel(HKEY hKey, PCWSTR subKey) {
    // Reading a label needs READ_CONTROL, writing one WRITE_OWNER.
    wil::unique_hkey key;
    LSTATUS status = RegOpenKeyEx(
        hKey, subKey, 0, KEY_WOW64_64KEY | READ_CONTROL | WRITE_OWNER, &key);
    if (status != ERROR_SUCCESS) {
        return status;
    }

    PACL sacl = nullptr;
    PSECURITY_DESCRIPTOR securityDescriptor = nullptr;
    DWORD error =
        GetSecurityInfo(key.get(), SE_REGISTRY_KEY, LABEL_SECURITY_INFORMATION,
                        nullptr, nullptr, nullptr, &sacl, &securityDescriptor);
    if (error != ERROR_SUCCESS) {
        return error;
    }
    wil::unique_hlocal_security_descriptor securityDescriptorOwner(
        securityDescriptor);

    if (!SaclContainsAnyLabel(sacl)) {
        return ERROR_SUCCESS;
    }

    alignas(DWORD) BYTE aclBuffer[sizeof(ACL)];
    PACL emptyAcl = reinterpret_cast<PACL>(aclBuffer);
    if (!InitializeAcl(emptyAcl, sizeof(aclBuffer), ACL_REVISION)) {
        return GetLastError();
    }

    return SetSecurityInfo(key.get(), SE_REGISTRY_KEY,
                           LABEL_SECURITY_INFORMATION, nullptr, nullptr,
                           nullptr, emptyAcl);
}

}  // namespace Functions
