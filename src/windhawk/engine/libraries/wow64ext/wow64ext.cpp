/**
 *
 * WOW64Ext Library
 *
 * Copyright (c) 2014 ReWolf
 * http://blog.rewolf.pl/
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Lesser General Public License as published
 * by the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Lesser General Public License for more details.
 *
 * You should have received a copy of the GNU Lesser General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 *
 */

#include <Windows.h>
#include <cstddef>
#include <mutex>
#include <utility>
#include "internal.h"
#include "wow64ext.h"

#ifndef STATUS_SUCCESS
#   define STATUS_SUCCESS 0
#endif

#pragma pack(push)
#pragma pack(1)

template <class T>
struct _LIST_ENTRY_T
{
    T Flink;
    T Blink;
};

template <class T>
struct _UNICODE_STRING_T
{
    union
    {
        struct
        {
            WORD Length;
            WORD MaximumLength;
        };
        T dummy;
    };
    T Buffer;
};

template <class T>
struct _NT_TIB_T
{
    T ExceptionList;
    T StackBase;
    T StackLimit;
    T SubSystemTib;
    T FiberData;
    T ArbitraryUserPointer;
    T Self;
};

template <class T>
struct _CLIENT_ID
{
    T UniqueProcess;
    T UniqueThread;
};

template <class T>
struct _TEB_T_
{
    _NT_TIB_T<T> NtTib;
    T EnvironmentPointer;
    _CLIENT_ID<T> ClientId;
    T ActiveRpcHandle;
    T ThreadLocalStoragePointer;
    T ProcessEnvironmentBlock;
    DWORD LastErrorValue;
    DWORD CountOfOwnedCriticalSections;
    T CsrClientThread;
    T Win32ThreadInfo;
    DWORD User32Reserved[26];
    //rest of the structure is not defined for now, as it is not needed
};

template <class T>
struct _LDR_DATA_TABLE_ENTRY_T
{
    _LIST_ENTRY_T<T> InLoadOrderLinks;
    _LIST_ENTRY_T<T> InMemoryOrderLinks;
    _LIST_ENTRY_T<T> InInitializationOrderLinks;
    T DllBase;
    T EntryPoint;
    union
    {
        DWORD SizeOfImage;
        T dummy01;
    };
    _UNICODE_STRING_T<T> FullDllName;
    _UNICODE_STRING_T<T> BaseDllName;
    DWORD Flags;
    WORD LoadCount;
    WORD TlsIndex;
    union
    {
        _LIST_ENTRY_T<T> HashLinks;
        struct
        {
            T SectionPointer;
            T CheckSum;
        };
    };
    union
    {
        T LoadedImports;
        DWORD TimeDateStamp;
    };
    T EntryPointActivationContext;
    T PatchInformation;
    _LIST_ENTRY_T<T> ForwarderLinks;
    _LIST_ENTRY_T<T> ServiceTagLinks;
    _LIST_ENTRY_T<T> StaticLinks;
    T ContextInformation;
    T OriginalBase;
    _LARGE_INTEGER LoadTime;
};

template <class T>
struct _PEB_LDR_DATA_T
{
    DWORD Length;
    DWORD Initialized;
    T SsHandle;
    _LIST_ENTRY_T<T> InLoadOrderModuleList;
    _LIST_ENTRY_T<T> InMemoryOrderModuleList;
    _LIST_ENTRY_T<T> InInitializationOrderModuleList;
    T EntryInProgress;
    DWORD ShutdownInProgress;
    T ShutdownThreadId;

};

template <class T, class NGF, int A>
struct _PEB_T
{
    union
    {
        struct
        {
            BYTE InheritedAddressSpace;
            BYTE ReadImageFileExecOptions;
            BYTE BeingDebugged;
            BYTE BitField;
        };
        T dummy01;
    };
    T Mutant;
    T ImageBaseAddress;
    T Ldr;
    T ProcessParameters;
    T SubSystemData;
    T ProcessHeap;
    T FastPebLock;
    T AtlThunkSListPtr;
    T IFEOKey;
    T CrossProcessFlags;
    T UserSharedInfoPtr;
    DWORD SystemReserved;
    DWORD AtlThunkSListPtr32;
    T ApiSetMap;
    T TlsExpansionCounter;
    T TlsBitmap;
    DWORD TlsBitmapBits[2];
    T ReadOnlySharedMemoryBase;
    T HotpatchInformation;
    T ReadOnlyStaticServerData;
    T AnsiCodePageData;
    T OemCodePageData;
    T UnicodeCaseTableData;
    DWORD NumberOfProcessors;
    union
    {
        DWORD NtGlobalFlag;
        NGF dummy02;
    };
    LARGE_INTEGER CriticalSectionTimeout;
    T HeapSegmentReserve;
    T HeapSegmentCommit;
    T HeapDeCommitTotalFreeThreshold;
    T HeapDeCommitFreeBlockThreshold;
    DWORD NumberOfHeaps;
    DWORD MaximumNumberOfHeaps;
    T ProcessHeaps;
    T GdiSharedHandleTable;
    T ProcessStarterHelper;
    T GdiDCAttributeList;
    T LoaderLock;
    DWORD OSMajorVersion;
    DWORD OSMinorVersion;
    WORD OSBuildNumber;
    WORD OSCSDVersion;
    DWORD OSPlatformId;
    DWORD ImageSubsystem;
    DWORD ImageSubsystemMajorVersion;
    T ImageSubsystemMinorVersion;
    T ActiveProcessAffinityMask;
    T GdiHandleBuffer[A];
    T PostProcessInitRoutine;
    T TlsExpansionBitmap;
    DWORD TlsExpansionBitmapBits[32];
    T SessionId;
    ULARGE_INTEGER AppCompatFlags;
    ULARGE_INTEGER AppCompatFlagsUser;
    T pShimData;
    T AppCompatInfo;
    _UNICODE_STRING_T<T> CSDVersion;
    T ActivationContextData;
    T ProcessAssemblyStorageMap;
    T SystemDefaultActivationContextData;
    T SystemAssemblyStorageMap;
    T MinimumStackCommit;
    T FlsCallback;
    _LIST_ENTRY_T<T> FlsListHead;
    T FlsBitmap;
    DWORD FlsBitmapBits[4];
    T FlsHighIndex;
    T WerRegistrationData;
    T WerShipAssertPtr;
    T pContextData;
    T pImageHeaderHash;
    T TracingFlags;
};

typedef _LDR_DATA_TABLE_ENTRY_T<DWORD> LDR_DATA_TABLE_ENTRY32;
typedef _LDR_DATA_TABLE_ENTRY_T<DWORD64> LDR_DATA_TABLE_ENTRY64;

typedef _TEB_T_<DWORD> TEB32;
typedef _TEB_T_<DWORD64> TEB64;

typedef _PEB_LDR_DATA_T<DWORD> PEB_LDR_DATA32;
typedef _PEB_LDR_DATA_T<DWORD64> PEB_LDR_DATA64;

typedef _PEB_T<DWORD, DWORD64, 34> PEB32;
typedef _PEB_T<DWORD64, DWORD, 30> PEB64;

#pragma pack(pop)

static HANDLE g_heap;
static BOOL g_isWow64;

static void* wow64ext_malloc(size_t size)
{
    return HeapAlloc(g_heap, 0, size);
}

static void wow64ext_free(void* ptr)
{
    if (nullptr != ptr)
        HeapFree(g_heap, 0, ptr);
}

class CMemPtr
{
private:
    void** m_ptr;
    bool watchActive;

public:
    CMemPtr(void** ptr) : m_ptr(ptr), watchActive(true) {}

    ~CMemPtr()
    {
        if (*m_ptr && watchActive)
        {
            wow64ext_free(*m_ptr);
            *m_ptr = 0;
        }
    }

    void disableWatch() { watchActive = false; }
};

#define WATCH(ptr) \
    CMemPtr watch_##ptr((void**)&ptr)

#define DISABLE_WATCH(ptr) \
    watch_##ptr.disableWatch()

// The following macros are used to initialize static variables once in a
// thread-safe manner while avoiding TLS, which is what MSVC uses for static
// variables.

// Similar to:
// static T var_name = initializer;
#define STATIC_INIT_ONCE_TRIVIAL(T, var_name, initializer)  \
    static T var_name;                                      \
    do {                                                    \
        static_assert(std::is_trivially_destructible_v<T>); \
        static std::once_flag static_init_once_flag_;       \
        std::call_once(static_init_once_flag_,              \
                       []() { var_name = initializer; });   \
    } while (0)

// Similar to:
// static T var_name = (T)GetProcAddress(module, proc_name);
#define GET_PROC_ADDRESS_ONCE(T, var_name, module, proc_name) \
    STATIC_INIT_ONCE_TRIVIAL(T, var_name, (T)GetProcAddress(module, proc_name))

// Similar to:
// static DWORD64 var_name = GetProcAddress64(getNTDLL64(), proc_name);
#define GET_PROC_ADDRESS_ONCE_NTDLL64(var_name, proc_name) \
    STATIC_INIT_ONCE_TRIVIAL(DWORD64, var_name, GetProcAddress64(getNTDLL64(), proc_name))

VOID Wow64ExtInitialize()
{
    IsWow64Process(GetCurrentProcess(), &g_isWow64);
    g_heap = GetProcessHeap();
}

#pragma warning(push)
#pragma warning(disable : 4409)
DWORD64 __cdecl X64Call(DWORD64 func, int argC, ...)
{
    if (!g_isWow64)
        return 0;

    va_list args;
    va_start(args, argC);
    reg64 _rcx = { (argC > 0) ? argC--, va_arg(args, DWORD64) : 0 };
    reg64 _rdx = { (argC > 0) ? argC--, va_arg(args, DWORD64) : 0 };
    reg64 _r8 = { (argC > 0) ? argC--, va_arg(args, DWORD64) : 0 };
    reg64 _r9 = { (argC > 0) ? argC--, va_arg(args, DWORD64) : 0 };
    reg64 _rax = { 0 };

    reg64 restArgs = { PTR_TO_DWORD64(&va_arg(args, DWORD64)) };

    // conversion to QWORD for easier use in inline assembly
    reg64 _argC = { (DWORD64)argC };
    DWORD back_esp = 0;
    WORD back_fs = 0;

    __asm
    {
        ;// reset FS segment, to properly handle RFG
        mov    back_fs, fs
        mov    eax, 0x2B
        mov    fs, ax

        ;// keep original esp in back_esp variable
        mov    back_esp, esp

        ;// align esp to 0x10, without aligned stack some syscalls may return errors !
        ;// (actually, for syscalls it is sufficient to align to 8, but SSE opcodes
        ;// requires 0x10 alignment), it will be further adjusted according to the
        ;// number of arguments above 4
        and    esp, 0xFFFFFFF0

        X64_Start();

        ;// below code is compiled as x86 inline asm, but it is executed as x64 code
        ;// that's why it need sometimes REX_W() macro, right column contains detailed
        ;// transcription how it will be interpreted by CPU

        ;// fill first four arguments
  REX_W mov    ecx, _rcx.dw[0]                          ;// mov     rcx, qword ptr [_rcx]
  REX_W mov    edx, _rdx.dw[0]                          ;// mov     rdx, qword ptr [_rdx]
        push   _r8.v                                    ;// push    qword ptr [_r8]
        X64_Pop(_R8);                                   ;// pop     r8
        push   _r9.v                                    ;// push    qword ptr [_r9]
        X64_Pop(_R9);                                   ;// pop     r9
                                                        ;//
  REX_W mov    eax, _argC.dw[0]                         ;// mov     rax, qword ptr [_argC]
                                                        ;//
        ;// final stack adjustment, according to the    ;//
        ;// number of arguments above 4                 ;//
        test   al, 1                                    ;// test    al, 1
        jnz    _no_adjust                               ;// jnz     _no_adjust
        sub    esp, 8                                   ;// sub     rsp, 8
_no_adjust:                                             ;//
                                                        ;//
        push   edi                                      ;// push    rdi
  REX_W mov    edi, restArgs.dw[0]                      ;// mov     rdi, qword ptr [restArgs]
                                                        ;//
        ;// put rest of arguments on the stack          ;//
  REX_W test   eax, eax                                 ;// test    rax, rax
        jz     _ls_e                                    ;// je      _ls_e
  REX_W lea    edi, dword ptr [edi + 8*eax - 8]         ;// lea     rdi, [rdi + rax*8 - 8]
                                                        ;//
_ls:                                                    ;//
  REX_W test   eax, eax                                 ;// test    rax, rax
        jz     _ls_e                                    ;// je      _ls_e
        push   dword ptr [edi]                          ;// push    qword ptr [rdi]
  REX_W sub    edi, 8                                   ;// sub     rdi, 8
  REX_W sub    eax, 1                                   ;// sub     rax, 1
        jmp    _ls                                      ;// jmp     _ls
_ls_e:                                                  ;//
                                                        ;//
        ;// create stack space for spilling registers   ;//
  REX_W sub    esp, 0x20                                ;// sub     rsp, 20h
                                                        ;//
        call   func                                     ;// call    qword ptr [func]
                                                        ;//
        ;// cleanup stack                               ;//
  REX_W mov    ecx, _argC.dw[0]                         ;// mov     rcx, qword ptr [_argC]
  REX_W lea    esp, dword ptr [esp + 8*ecx + 0x20]      ;// lea     rsp, [rsp + rcx*8 + 20h]
                                                        ;//
        pop    edi                                      ;// pop     rdi
                                                        ;//
        // set return value                             ;//
  REX_W mov    _rax.dw[0], eax                          ;// mov     qword ptr [_rax], rax

        X64_End();

        mov    ax, ds
        mov    ss, ax
        mov    esp, back_esp

        ;// restore FS segment
        mov    ax, back_fs
        mov    fs, ax
    }
    return _rax.v;
}
#pragma warning(pop)

static void getMem64(void* dstMem, DWORD64 srcMem, size_t sz)
{
    if ((nullptr == dstMem) || (0 == srcMem) || (0 == sz))
        return;

    reg64 _src = { srcMem };

    __asm
    {
        X64_Start();

        ;// below code is compiled as x86 inline asm, but it is executed as x64 code
        ;// that's why it need sometimes REX_W() macro, right column contains detailed
        ;// transcription how it will be interpreted by CPU

        push   edi                  ;// push     rdi
        push   esi                  ;// push     rsi
                                    ;//
        mov    edi, dstMem          ;// mov      edi, dword ptr [dstMem]        ; high part of RDI is zeroed
  REX_W mov    esi, _src.dw[0]      ;// mov      rsi, qword ptr [_src]
        mov    ecx, sz              ;// mov      ecx, dword ptr [sz]            ; high part of RCX is zeroed
                                    ;//
        mov    eax, ecx             ;// mov      eax, ecx
        and    eax, 3               ;// and      eax, 3
        shr    ecx, 2               ;// shr      ecx, 2
                                    ;//
        rep    movsd                ;// rep movs dword ptr [rdi], dword ptr [rsi]
                                    ;//
        test   eax, eax             ;// test     eax, eax
        je     _move_0              ;// je       _move_0
        cmp    eax, 1               ;// cmp      eax, 1
        je     _move_1              ;// je       _move_1
                                    ;//
        movsw                       ;// movs     word ptr [rdi], word ptr [rsi]
        cmp    eax, 2               ;// cmp      eax, 2
        je     _move_0              ;// je       _move_0
                                    ;//
_move_1:                            ;//
        movsb                       ;// movs     byte ptr [rdi], byte ptr [rsi]
                                    ;//
_move_0:                            ;//
        pop    esi                  ;// pop      rsi
        pop    edi                  ;// pop      rdi

        X64_End();
    }
}

static bool cmpMem64(const void* dstMem, DWORD64 srcMem, size_t sz)
{
    if ((nullptr == dstMem) || (0 == srcMem) || (0 == sz))
        return false;

    bool result = false;
    reg64 _src = { srcMem };
    __asm
    {
        X64_Start();

        ;// below code is compiled as x86 inline asm, but it is executed as x64 code
        ;// that's why it need sometimes REX_W() macro, right column contains detailed
        ;// transcription how it will be interpreted by CPU

        push   edi                  ;// push      rdi
        push   esi                  ;// push      rsi
                                    ;//
        mov    edi, dstMem          ;// mov       edi, dword ptr [dstMem]       ; high part of RDI is zeroed
  REX_W mov    esi, _src.dw[0]      ;// mov       rsi, qword ptr [_src]
        mov    ecx, sz              ;// mov       ecx, dword ptr [sz]           ; high part of RCX is zeroed
                                    ;//
        mov    eax, ecx             ;// mov       eax, ecx
        and    eax, 3               ;// and       eax, 3
        shr    ecx, 2               ;// shr       ecx, 2
                                    ;//
        repe   cmpsd                ;// repe cmps dword ptr [rsi], dword ptr [rdi]
        jnz     _ret_false          ;// jnz       _ret_false
                                    ;//
        test   eax, eax             ;// test      eax, eax
        je     _move_0              ;// je        _move_0
        cmp    eax, 1               ;// cmp       eax, 1
        je     _move_1              ;// je        _move_1
                                    ;//
        cmpsw                       ;// cmps      word ptr [rsi], word ptr [rdi]
        jnz     _ret_false          ;// jnz       _ret_false
        cmp    eax, 2               ;// cmp       eax, 2
        je     _move_0              ;// je        _move_0
                                    ;//
_move_1:                            ;//
        cmpsb                       ;// cmps      byte ptr [rsi], byte ptr [rdi]
        jnz     _ret_false          ;// jnz       _ret_false
                                    ;//
_move_0:                            ;//
        mov    result, 1            ;// mov       byte ptr [result], 1
                                    ;//
_ret_false:                         ;//
        pop    esi                  ;// pop      rsi
        pop    edi                  ;// pop      rdi

        X64_End();
    }

    return result;
}

static DWORD64 getTEB64()
{
    reg64 reg;
    reg.v = 0;

    X64_Start();
    // R12 register should always contain pointer to TEB64 in WoW64 processes
    X64_Push(_R12);
    // below pop will pop QWORD from stack, as we're in x64 mode now
    __asm pop reg.dw[0]
    X64_End();

    return reg.v;
}

DWORD64 __cdecl GetModuleHandle64(const wchar_t* lpModuleName)
{
    if (!g_isWow64)
        return 0;

    TEB64 teb64;
    getMem64(&teb64, getTEB64(), sizeof(TEB64));

    PEB64 peb64;
    getMem64(&peb64, teb64.ProcessEnvironmentBlock, sizeof(PEB64));
    PEB_LDR_DATA64 ldr;
    getMem64(&ldr, peb64.Ldr, sizeof(PEB_LDR_DATA64));

    DWORD64 LastEntry = peb64.Ldr + offsetof(PEB_LDR_DATA64, InLoadOrderModuleList);
    LDR_DATA_TABLE_ENTRY64 head;
    head.InLoadOrderLinks.Flink = ldr.InLoadOrderModuleList.Flink;
    do
    {
        getMem64(&head, head.InLoadOrderLinks.Flink, sizeof(LDR_DATA_TABLE_ENTRY64));

        wchar_t* tempBuf = (wchar_t*)wow64ext_malloc(head.BaseDllName.MaximumLength);
        if (nullptr == tempBuf)
            return 0;
        WATCH(tempBuf);
        getMem64(tempBuf, head.BaseDllName.Buffer, head.BaseDllName.MaximumLength);

        if (0 == _wcsicmp(lpModuleName, tempBuf))
            return head.DllBase;
    }
    while (head.InLoadOrderLinks.Flink != LastEntry);

    return 0;
}

static DWORD64 getNTDLL64()
{
    STATIC_INIT_ONCE_TRIVIAL(DWORD64, ntdll64, GetModuleHandle64(L"ntdll.dll"));
    return ntdll64;
}

static DWORD64 getLdrGetProcedureAddress()
{
    DWORD64 modBase = getNTDLL64();
    if (0 == modBase)
        return 0;

    IMAGE_DOS_HEADER idh;
    getMem64(&idh, modBase, sizeof(idh));

    IMAGE_NT_HEADERS64 inh;
    getMem64(&inh, modBase + idh.e_lfanew, sizeof(IMAGE_NT_HEADERS64));

    IMAGE_DATA_DIRECTORY& idd = inh.OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_EXPORT];

    if (0 == idd.VirtualAddress)
        return 0;

    IMAGE_EXPORT_DIRECTORY ied;
    getMem64(&ied, modBase + idd.VirtualAddress, sizeof(ied));

    DWORD* rvaTable = (DWORD*)wow64ext_malloc(sizeof(DWORD)*ied.NumberOfFunctions);
    if (nullptr == rvaTable)
        return 0;
    WATCH(rvaTable);
    getMem64(rvaTable, modBase + ied.AddressOfFunctions, sizeof(DWORD)*ied.NumberOfFunctions);

    WORD* ordTable = (WORD*)wow64ext_malloc(sizeof(WORD)*ied.NumberOfFunctions);
    if (nullptr == ordTable)
        return 0;
    WATCH(ordTable);
    getMem64(ordTable, modBase + ied.AddressOfNameOrdinals, sizeof(WORD)*ied.NumberOfFunctions);

    DWORD* nameTable = (DWORD*)wow64ext_malloc(sizeof(DWORD)*ied.NumberOfNames);
    if (nullptr == nameTable)
        return 0;
    WATCH(nameTable);
    getMem64(nameTable, modBase + ied.AddressOfNames, sizeof(DWORD)*ied.NumberOfNames);

    // lazy search, there is no need to use binsearch for just one function
    for (DWORD i = 0; i < ied.NumberOfFunctions; i++)
    {
        if (!cmpMem64("LdrGetProcedureAddress", modBase + nameTable[i], sizeof("LdrGetProcedureAddress")))
            continue;
        else
            return modBase + rvaTable[ordTable[i]];
    }
    return 0;
}

VOID __cdecl SetLastErrorFromX64Call(DWORD64 status)
{
    typedef ULONG (WINAPI *RtlNtStatusToDosError_t)(NTSTATUS Status);
    typedef ULONG (WINAPI *RtlSetLastWin32Error_t)(NTSTATUS Status);

    STATIC_INIT_ONCE_TRIVIAL(HMODULE, ntdll, GetModuleHandleW(L"ntdll.dll"));
    GET_PROC_ADDRESS_ONCE(RtlNtStatusToDosError_t, RtlNtStatusToDosError, ntdll, "RtlNtStatusToDosError");
    GET_PROC_ADDRESS_ONCE(RtlSetLastWin32Error_t, RtlSetLastWin32Error, ntdll, "RtlSetLastWin32Error");

    if ((nullptr != RtlNtStatusToDosError) && (nullptr != RtlSetLastWin32Error))
    {
        RtlSetLastWin32Error(RtlNtStatusToDosError((DWORD)status));
    }
}

DWORD64 __cdecl GetProcAddress64(DWORD64 hModule, const char* funcName)
{
    STATIC_INIT_ONCE_TRIVIAL(DWORD64, _LdrGetProcedureAddress, getLdrGetProcedureAddress());
    if (0 == _LdrGetProcedureAddress)
        return 0;

    _UNICODE_STRING_T<DWORD64> fName = { 0 };
    fName.Buffer = PTR_TO_DWORD64(funcName);
    fName.Length = (WORD)strlen(funcName);
    fName.MaximumLength = fName.Length + 1;
    DWORD64 funcRet = 0;
    X64Call(_LdrGetProcedureAddress, 4, hModule, PTR_TO_DWORD64(&fName), (DWORD64)0, PTR_TO_DWORD64(&funcRet));
    return funcRet;
}

SIZE_T __cdecl VirtualQueryEx64(HANDLE hProcess, DWORD64 lpAddress, MEMORY_BASIC_INFORMATION64* lpBuffer, SIZE_T dwLength)
{
    GET_PROC_ADDRESS_ONCE_NTDLL64(ntqvm, "NtQueryVirtualMemory");
    if (0 == ntqvm)
        return 0;

    DWORD64 ret = 0;
    DWORD64 status = X64Call(ntqvm, 6, HANDLE_TO_DWORD64(hProcess), lpAddress, (DWORD64)0, PTR_TO_DWORD64(lpBuffer), (DWORD64)dwLength, PTR_TO_DWORD64(&ret));
    if (STATUS_SUCCESS != status)
        SetLastErrorFromX64Call(status);
    return (SIZE_T)ret;
}

DWORD64 __cdecl VirtualAllocEx64(HANDLE hProcess, DWORD64 lpAddress, SIZE_T dwSize, DWORD flAllocationType, DWORD flProtect)
{
    GET_PROC_ADDRESS_ONCE_NTDLL64(ntavm, "NtAllocateVirtualMemory");
    if (0 == ntavm)
        return 0;

    DWORD64 tmpAddr = lpAddress;
    DWORD64 tmpSize = dwSize;
    DWORD64 ret = X64Call(ntavm, 6, HANDLE_TO_DWORD64(hProcess), PTR_TO_DWORD64(&tmpAddr), (DWORD64)0, PTR_TO_DWORD64(&tmpSize), (DWORD64)flAllocationType, (DWORD64)flProtect);
    if (STATUS_SUCCESS != ret)
    {
        SetLastErrorFromX64Call(ret);
        return 0;
    }
    else
        return tmpAddr;
}

BOOL __cdecl VirtualFreeEx64(HANDLE hProcess, DWORD64 lpAddress, SIZE_T dwSize, DWORD dwFreeType)
{
    GET_PROC_ADDRESS_ONCE_NTDLL64(ntfvm, "NtFreeVirtualMemory");
    if (0 == ntfvm)
        return FALSE;

    DWORD64 tmpAddr = lpAddress;
    DWORD64 tmpSize = dwSize;
    DWORD64 ret = X64Call(ntfvm, 4, HANDLE_TO_DWORD64(hProcess), PTR_TO_DWORD64(&tmpAddr), PTR_TO_DWORD64(&tmpSize), (DWORD64)dwFreeType);
    if (STATUS_SUCCESS != ret)
    {
        SetLastErrorFromX64Call(ret);
        return FALSE;
    }
    else
        return TRUE;
}

BOOL __cdecl VirtualProtectEx64(HANDLE hProcess, DWORD64 lpAddress, SIZE_T dwSize, DWORD flNewProtect, DWORD* lpflOldProtect)
{
    GET_PROC_ADDRESS_ONCE_NTDLL64(ntpvm, "NtProtectVirtualMemory");
    if (0 == ntpvm)
        return FALSE;

    DWORD64 tmpAddr = lpAddress;
    DWORD64 tmpSize = dwSize;
    DWORD64 ret = X64Call(ntpvm, 5, HANDLE_TO_DWORD64(hProcess), PTR_TO_DWORD64(&tmpAddr), PTR_TO_DWORD64(&tmpSize), (DWORD64)flNewProtect, PTR_TO_DWORD64(lpflOldProtect));
    if (STATUS_SUCCESS != ret)
    {
        SetLastErrorFromX64Call(ret);
        return FALSE;
    }
    else
        return TRUE;
}

BOOL __cdecl ReadProcessMemory64(HANDLE hProcess, DWORD64 lpBaseAddress, LPVOID lpBuffer, SIZE_T nSize, SIZE_T* lpNumberOfBytesRead)
{
    GET_PROC_ADDRESS_ONCE_NTDLL64(nrvm, "NtReadVirtualMemory");
    if (0 == nrvm)
        return FALSE;

    DWORD64 numOfBytes = lpNumberOfBytesRead ? *lpNumberOfBytesRead : 0;
    DWORD64 ret = X64Call(nrvm, 5, HANDLE_TO_DWORD64(hProcess), lpBaseAddress, PTR_TO_DWORD64(lpBuffer), (DWORD64)nSize, PTR_TO_DWORD64(&numOfBytes));
    if (STATUS_SUCCESS != ret)
    {
        SetLastErrorFromX64Call(ret);
        return FALSE;
    }
    else
    {
        if (lpNumberOfBytesRead)
            *lpNumberOfBytesRead = (SIZE_T)numOfBytes;
        return TRUE;
    }
}

BOOL __cdecl WriteProcessMemory64(HANDLE hProcess, DWORD64 lpBaseAddress, LPCVOID lpBuffer, SIZE_T nSize, SIZE_T* lpNumberOfBytesWritten)
{
    GET_PROC_ADDRESS_ONCE_NTDLL64(nrvm, "NtWriteVirtualMemory");
    if (0 == nrvm)
        return FALSE;

    DWORD64 numOfBytes = lpNumberOfBytesWritten ? *lpNumberOfBytesWritten : 0;
    DWORD64 ret = X64Call(nrvm, 5, HANDLE_TO_DWORD64(hProcess), lpBaseAddress, PTR_TO_DWORD64(lpBuffer), (DWORD64)nSize, PTR_TO_DWORD64(&numOfBytes));
    if (STATUS_SUCCESS != ret)
    {
        SetLastErrorFromX64Call(ret);
        return FALSE;
    }
    else
    {
        if (lpNumberOfBytesWritten)
            *lpNumberOfBytesWritten = (SIZE_T)numOfBytes;
        return TRUE;
    }
}

BOOL __cdecl GetThreadContext64(HANDLE hThread, _CONTEXT64* lpContext)
{
    GET_PROC_ADDRESS_ONCE_NTDLL64(gtc, "NtGetContextThread");
    if (0 == gtc)
        return FALSE;

    DWORD64 ret = X64Call(gtc, 2, HANDLE_TO_DWORD64(hThread), PTR_TO_DWORD64(lpContext));
    if (STATUS_SUCCESS != ret)
    {
        SetLastErrorFromX64Call(ret);
        return FALSE;
    }
    else
        return TRUE;
}

BOOL __cdecl SetThreadContext64(HANDLE hThread, const _CONTEXT64* lpContext)
{
    GET_PROC_ADDRESS_ONCE_NTDLL64(stc, "NtSetContextThread");
    if (0 == stc)
        return FALSE;

    DWORD64 ret = X64Call(stc, 2, HANDLE_TO_DWORD64(hThread), PTR_TO_DWORD64(lpContext));
    if (STATUS_SUCCESS != ret)
    {
        SetLastErrorFromX64Call(ret);
        return FALSE;
    }
    else
        return TRUE;
}

DWORD64 __cdecl LoadLibraryW64(LPCWSTR lpLibFileName)
{
    GET_PROC_ADDRESS_ONCE_NTDLL64(ldrLoadDll, "LdrLoadDll");
    if (0 == ldrLoadDll)
        return 0;

    _UNICODE_STRING_T<DWORD64> szDll = { 0 };
    szDll.Buffer = PTR_TO_DWORD64(lpLibFileName);
    szDll.Length = (WORD)(wcslen(lpLibFileName) * sizeof(WCHAR));
    szDll.MaximumLength = szDll.Length + sizeof(WCHAR);
    DWORD64 hModuleHandle = 0;
    DWORD64 ret = X64Call(ldrLoadDll, 4, (DWORD64)0, (DWORD64)0, PTR_TO_DWORD64(&szDll), PTR_TO_DWORD64(&hModuleHandle));
    if (STATUS_SUCCESS != ret)
    {
        SetLastErrorFromX64Call(ret);
        return 0;
    }
    else
        return hModuleHandle;
}

DWORD64 __cdecl CreateRemoteThread64(DWORD64 hProcess, DWORD64 remote_addr, DWORD64 thread_arg, DWORD create_flags)
{
    GET_PROC_ADDRESS_ONCE_NTDLL64(nrvm, "NtCreateThreadEx");
    if (0 == nrvm)
        return 0;

    DWORD64 thread_handle = 0;

    DWORD64 ret = X64Call(nrvm, 11,
        PTR_TO_DWORD64(&thread_handle),
        (DWORD64)THREAD_ALL_ACCESS,
        (DWORD64)nullptr,
        hProcess,
        remote_addr,
        thread_arg,
        (DWORD64)create_flags,
        (DWORD64)0,
        (DWORD64)0,
        (DWORD64)0,
        (DWORD64)nullptr
    );
    if (STATUS_SUCCESS != ret)
    {
        SetLastErrorFromX64Call(ret);
        return 0;
    }
    else
        return thread_handle;
}

BOOL __cdecl CloseHandle64(DWORD64 Handle)
{
    GET_PROC_ADDRESS_ONCE_NTDLL64(nrvm, "NtClose");
    if (0 == nrvm)
        return FALSE;

    DWORD64 ret = X64Call(nrvm, 1, Handle);
    if (STATUS_SUCCESS != ret)
    {
        SetLastErrorFromX64Call(ret);
        return FALSE;
    }
    else
        return TRUE;
}

BOOL __cdecl NtQueueApcThread64(DWORD64 ThreadHandle, DWORD64 ApcDispatchRoutine, DWORD64 SystemArgument1, DWORD64 SystemArgument2, DWORD64 SystemArgument3)
{
    GET_PROC_ADDRESS_ONCE_NTDLL64(nrvm, "NtQueueApcThread");
    if (0 == nrvm)
        return FALSE;

    DWORD64 ret = X64Call(nrvm, 5,
        ThreadHandle,
        ApcDispatchRoutine,
        SystemArgument1,
        SystemArgument2,
        SystemArgument3
    );
    if (STATUS_SUCCESS != ret)
    {
        SetLastErrorFromX64Call(ret);
        return FALSE;
    }
    else
        return TRUE;
}
