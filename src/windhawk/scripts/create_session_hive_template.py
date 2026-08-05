# Builds the template application hive that backs the session metadata store and
# writes it to engine/session_metadata_hive_template.h, from where the engine
# writes it out as a file. Run manually; the result is a static buffer that only
# needs rebuilding when the security descriptor below changes.
#
# The store normally lives under HKEY_LOCAL_MACHINE. A portable session manager
# without administrative rights can't create that key and puts the store in an
# application hive instead, which the engine creates from the template on
# startup. A hive that RegLoadAppKey creates on its own carries a Low integrity
# label, which the mandatory No-Write-Up policy turns into a write denial for
# every engine running below Low, and the label can't be changed afterwards: all
# keys in an application hive share the hive's own security descriptor, and both
# RegSetKeySecurity and per-key descriptors at creation are refused. The
# descriptor is therefore fixed here, when the hive is built.

import ctypes
import shutil
import tempfile
from ctypes import wintypes
from pathlib import Path

HEADER_PATH = Path(__file__).resolve().parent.parent / 'engine' / \
    'session_metadata_hive_template.h'

BUFFER_NAME = 'kSessionMetadataHiveTemplate'

BYTES_PER_LINE = 12

# The hive format version. Windows 10 is the oldest version Windhawk 2.0
# supports, so nothing older needs to be able to load this.
HIVE_OS_MAJOR = 10
HIVE_OS_MINOR = 0

# KEY_QUERY_VALUE | KEY_SET_VALUE | KEY_CREATE_SUB_KEY |
# KEY_ENUMERATE_SUB_KEYS | KEY_NOTIFY | DELETE | READ_CONTROL.
#
# Every right the session manager needs has to be granted to the shared SIDs:
# the descriptor covers the whole hive, so it can't name the session manager
# apart from the engines writing to it. KEY_CREATE_LINK is withheld to keep
# registry symbolic links out of a world-writable area, and WRITE_DAC and
# WRITE_OWNER because the grantees include sandboxed and low-integrity
# processes.
STORE_ACCESS = 0x0003001F

# Full control for SYSTEM and Administrators, the access above for Everyone and
# the two shared sandbox SIDs, all inheritable so the session and category keys
# created inside the hive carry the same descriptor. The Untrusted integrity
# label is the point of the exercise: it admits writers at any integrity level,
# where the label of a self-created hive admits only Low and above.
TEMPLATE_SDDL = (
    'O:BAG:BA'
    'D:P(A;CI;KA;;;SY)(A;CI;KA;;;BA)'
    f'(A;CI;0x{STORE_ACCESS:08X};;;WD)'
    f'(A;CI;0x{STORE_ACCESS:08X};;;S-1-15-2-1)'
    f'(A;CI;0x{STORE_ACCESS:08X};;;S-1-15-2-2)'
    'S:(ML;CI;NW;;;S-1-16-0)'
)

SDDL_REVISION_1 = 1

OWNER_SECURITY_INFORMATION = 0x00000001
GROUP_SECURITY_INFORMATION = 0x00000002
DACL_SECURITY_INFORMATION = 0x00000004
LABEL_SECURITY_INFORMATION = 0x00000010

KEY_READ = 0x00020019
KEY_WRITE = 0x00020006

ERROR_SUCCESS = 0

advapi32 = ctypes.WinDLL('advapi32', use_last_error=True)
kernel32 = ctypes.WinDLL('kernel32', use_last_error=True)
offreg = ctypes.WinDLL('offreg', use_last_error=True)

ORHKEY = ctypes.c_void_p

offreg.ORCreateHive.argtypes = [ctypes.POINTER(ORHKEY)]
offreg.ORCreateHive.restype = wintypes.DWORD
offreg.ORSetKeySecurity.argtypes = [ORHKEY, wintypes.DWORD, ctypes.c_void_p]
offreg.ORSetKeySecurity.restype = wintypes.DWORD
offreg.ORSaveHive.argtypes = [ORHKEY, wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD]
offreg.ORSaveHive.restype = wintypes.DWORD
offreg.ORCloseHive.argtypes = [ORHKEY]
offreg.ORCloseHive.restype = wintypes.DWORD

advapi32.ConvertStringSecurityDescriptorToSecurityDescriptorW.argtypes = [
    wintypes.LPCWSTR, wintypes.DWORD, ctypes.POINTER(ctypes.c_void_p),
    ctypes.POINTER(wintypes.ULONG)]
advapi32.ConvertStringSecurityDescriptorToSecurityDescriptorW.restype = wintypes.BOOL
advapi32.ConvertSecurityDescriptorToStringSecurityDescriptorW.argtypes = [
    ctypes.c_void_p, wintypes.DWORD, wintypes.DWORD,
    ctypes.POINTER(wintypes.LPWSTR), ctypes.POINTER(wintypes.ULONG)]
advapi32.ConvertSecurityDescriptorToStringSecurityDescriptorW.restype = wintypes.BOOL
advapi32.RegLoadAppKeyW.argtypes = [
    wintypes.LPCWSTR, ctypes.POINTER(wintypes.HKEY), wintypes.DWORD,
    wintypes.DWORD, wintypes.DWORD]
advapi32.RegLoadAppKeyW.restype = wintypes.LONG
advapi32.RegGetKeySecurity.argtypes = [
    wintypes.HKEY, wintypes.DWORD, ctypes.c_void_p, ctypes.POINTER(wintypes.DWORD)]
advapi32.RegGetKeySecurity.restype = wintypes.LONG
advapi32.RegCloseKey.argtypes = [wintypes.HKEY]
advapi32.RegCloseKey.restype = wintypes.LONG
kernel32.LocalFree.argtypes = [ctypes.c_void_p]
kernel32.LocalFree.restype = ctypes.c_void_p


def check(error: int, what: str):
    if error != ERROR_SUCCESS:
        raise OSError(f'{what} failed: {error}')


def security_descriptor_from_sddl(sddl: str) -> ctypes.c_void_p:
    descriptor = ctypes.c_void_p()
    if not advapi32.ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl, SDDL_REVISION_1, ctypes.byref(descriptor), None):
        raise ctypes.WinError(ctypes.get_last_error())
    return descriptor


def sddl_from_security_descriptor(descriptor: ctypes.c_void_p, info: int) -> str:
    string = wintypes.LPWSTR()
    if not advapi32.ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor, SDDL_REVISION_1, info, ctypes.byref(string), None):
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        return string.value
    finally:
        kernel32.LocalFree(string)


def create_template(target_path: Path):
    descriptor = security_descriptor_from_sddl(TEMPLATE_SDDL)
    try:
        hive = ORHKEY()
        check(offreg.ORCreateHive(ctypes.byref(hive)), 'ORCreateHive')
        try:
            check(offreg.ORSetKeySecurity(
                hive,
                OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION |
                DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION,
                descriptor), 'ORSetKeySecurity')

            # ORSaveHive won't overwrite.
            target_path.unlink(missing_ok=True)
            target_path.parent.mkdir(parents=True, exist_ok=True)

            check(offreg.ORSaveHive(hive, str(target_path.resolve()),
                                    HIVE_OS_MAJOR, HIVE_OS_MINOR), 'ORSaveHive')
        finally:
            offreg.ORCloseHive(hive)
    finally:
        kernel32.LocalFree(descriptor)


def verify_template(target_path: Path):
    # Load a copy rather than the artifact itself: loading a hive creates its
    # .LOG1 and .LOG2 recovery companions next to it and rewrites its header,
    # and the header should carry the bytes as they were built.
    with tempfile.TemporaryDirectory() as temp_dir:
        temp_path = Path(temp_dir, target_path.name)
        shutil.copy2(target_path, temp_path)
        verify_hive_file(temp_path)


def verify_hive_file(hive_path: Path):
    # Load it the way the session manager will, and read back what the registry
    # made of the descriptor.
    hive = wintypes.HKEY()
    check(advapi32.RegLoadAppKeyW(str(hive_path.resolve()), ctypes.byref(hive),
                                  KEY_READ | KEY_WRITE, 0, 0), 'RegLoadAppKey')
    try:
        info = (OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION |
                DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION)
        size = wintypes.DWORD(0)
        advapi32.RegGetKeySecurity(hive, info, None, ctypes.byref(size))
        buffer = ctypes.create_string_buffer(size.value)
        check(advapi32.RegGetKeySecurity(hive, info, buffer, ctypes.byref(size)),
              'RegGetKeySecurity')

        sddl = sddl_from_security_descriptor(
            ctypes.cast(buffer, ctypes.c_void_p), info)
        print(f'Loaded descriptor: {sddl}')

        # The label is what the whole template exists for, so a hive without it
        # is worse than none: it would look like it works everywhere except in
        # the sandboxed processes it was meant to serve.
        if '(ML;CI;NW;;;S-1-16-0)' not in sddl:
            raise Exception('The hive is missing the Untrusted integrity label')
    finally:
        advapi32.RegCloseKey(hive)


def write_header(header_path: Path, hive: bytes):
    lines = [
        '#pragma once',
        '',
        '#include <cstdint>',
        '',
        '// The template application hive backing the session metadata store, which',
        '// the session manager writes out as a file. See shared/session_metadata.h.',
        '//',
        '// Generated by scripts/create_session_hive_template.py, which also documents',
        '// the security descriptor built into it. Don\'t edit by hand.',
        f'inline constexpr uint8_t {BUFFER_NAME}[] = {{',
    ]

    for offset in range(0, len(hive), BYTES_PER_LINE):
        chunk = hive[offset:offset + BYTES_PER_LINE]
        lines.append('    ' + ' '.join(f'0x{byte:02X},' for byte in chunk))

    lines.append('};')
    lines.append('')

    with open(header_path, 'w', encoding='ascii', newline='\r\n') as f:
        f.write('\n'.join(lines))


def main():
    print(f'Descriptor: {TEMPLATE_SDDL}')

    with tempfile.TemporaryDirectory() as temp_dir:
        hive_path = Path(temp_dir, 'sessions-template.hiv')

        create_template(hive_path)
        print(f'Built the hive ({hive_path.stat().st_size} bytes)')

        verify_template(hive_path)
        print('Verified')

        write_header(HEADER_PATH, hive_path.read_bytes())
        print(f'Wrote {HEADER_PATH}')


if __name__ == '__main__':
    main()
