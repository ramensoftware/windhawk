# Script file from https://simonuvarov.com/msfvenom-reverse-tcp-waitforsingleobject/
# use to find the hash of Windows API calls. These hashes are used in Metasploit shellcode.

def ror(dword, bits):
    return (dword >> bits | dword << (32 - bits)) & 0xFFFFFFFF


def unicode(string, uppercase=True):
    result = ""
    if uppercase:
        string = string.upper()
    for c in string:
        result += c + "\x00"
    return result


def hash(function, bits=13, print_hash=True):
    function_hash = 0
    for c in function:
        function_hash = ror(function_hash, bits)
        function_hash += ord(c)
        function_hash &= 0xFFFFFFFF
    if print_hash:
        print("[+] 0x%08X = %s" % (function_hash, function))
    return function_hash


def module_hash(module, bits=13, print_hash=True):
    return hash(unicode(module), bits, print_hash)


if __name__ == '__main__':
    module_hash('kernel32.dll')
    hash('LoadLibraryW')
    module_hash('ntdll.dll')
    hash('LdrRegisterDllNotification')
    hash('LdrUnregisterDllNotification')
