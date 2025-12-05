//===============================================================================================//
// The code is based on code from the ReflectiveDLLInjection project:
// https://github.com/stephenfewer/ReflectiveDLLInjection/tree/master/dll/src
// Original license can be found below.
//===============================================================================================//
// Copyright (c) 2012, Stephen Fewer of Harmony Security (www.harmonysecurity.com)
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without modification, are permitted
// provided that the following conditions are met:
//
//     * Redistributions of source code must retain the above copyright notice, this list of
// conditions and the following disclaimer.
//
//     * Redistributions in binary form must reproduce the above copyright notice, this list of
// conditions and the following disclaimer in the documentation and/or other materials provided
// with the distribution.
//
//     * Neither the name of Harmony Security nor the names of its contributors may be used to
// endorse or promote products derived from this software without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR
// IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND
// FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER OR
// CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
// CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
// SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
// THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR
// OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
// POSSIBILITY OF SUCH DAMAGE.
//===============================================================================================//

#include <windows.h>

#include <iterator>

#include "../dll_inject.h"

// #define MESSAGE_BOX_TEST

#define DEREF(name)    *(UINT_PTR*)(name)
#define DEREF_64(name) *(DWORD64*)(name)
#define DEREF_32(name) *(DWORD*)(name)
#define DEREF_16(name) *(WORD*)(name)
#define DEREF_8(name)  *(BYTE*)(name)

//===============================================================================================//
typedef struct _UNICODE_STR
{
	USHORT Length;
	USHORT MaximumLength;
	PWSTR pBuffer;
} UNICODE_STR, *PUNICODE_STR;

// WinDbg> dt -v ntdll!_LDR_DATA_TABLE_ENTRY
//__declspec( align(8) )
typedef struct _LDR_DATA_TABLE_ENTRY
{
	LIST_ENTRY InLoadOrderLinks;
	LIST_ENTRY InMemoryOrderModuleList;
	LIST_ENTRY InInitializationOrderModuleList;
	PVOID DllBase;
	PVOID EntryPoint;
	ULONG SizeOfImage;
	UNICODE_STR FullDllName;
	UNICODE_STR BaseDllName;
	ULONG Flags;
	SHORT LoadCount;
	SHORT TlsIndex;
	LIST_ENTRY HashTableEntry;
	ULONG TimeDateStamp;
} LDR_DATA_TABLE_ENTRY, *PLDR_DATA_TABLE_ENTRY;

// WinDbg> dt -v ntdll!_PEB_LDR_DATA
typedef struct _PEB_LDR_DATA //, 7 elements, 0x28 bytes
{
	DWORD dwLength;
	DWORD dwInitialized;
	LPVOID lpSsHandle;
	LIST_ENTRY InLoadOrderModuleList;
	LIST_ENTRY InMemoryOrderModuleList;
	LIST_ENTRY InInitializationOrderModuleList;
	LPVOID lpEntryInProgress;
} PEB_LDR_DATA, *PPEB_LDR_DATA;

// WinDbg> dt -v ntdll!_PEB_FREE_BLOCK
typedef struct _PEB_FREE_BLOCK // 2 elements, 0x8 bytes
{
	struct _PEB_FREE_BLOCK* pNext;
	DWORD dwSize;
} PEB_FREE_BLOCK, *PPEB_FREE_BLOCK;

// struct _PEB is defined in Winternl.h but it is incomplete
// https://ntdoc.m417z.com/peb
typedef struct __PEB
{
	BOOLEAN InheritedAddressSpace;
	BOOLEAN ReadImageFileExecOptions;
	BOOLEAN BeingDebugged;
	union {
		BOOLEAN BitField;
		struct
		{
			BOOLEAN ImageUsesLargePages : 1;
			BOOLEAN IsProtectedProcess : 1;
			BOOLEAN IsImageDynamicallyRelocated : 1;
			BOOLEAN SkipPatchingUser32Forwarders : 1;
			BOOLEAN IsPackagedProcess : 1;
			BOOLEAN IsAppContainer : 1;
			BOOLEAN IsProtectedProcessLight : 1;
			BOOLEAN IsLongPathAwareProcess : 1;
		};
	};
	HANDLE Mutant;
	PVOID ImageBaseAddress;
	PPEB_LDR_DATA Ldr;
	/*PRTL_USER_PROCESS_PARAMETERS*/ PVOID ProcessParameters;
	PVOID SubSystemData;
	PVOID ProcessHeap;
	PRTL_CRITICAL_SECTION FastPebLock;
	PSLIST_HEADER AtlThunkSListPtr;
	PVOID IFEOKey;
	union {
		ULONG CrossProcessFlags;
		struct
		{
			ULONG ProcessInJob : 1;
			ULONG ProcessInitializing : 1;
			ULONG ProcessUsingVEH : 1;
			ULONG ProcessUsingVCH : 1;
			ULONG ProcessUsingFTH : 1;
			ULONG ProcessPreviouslyThrottled : 1;
			ULONG ProcessCurrentlyThrottled : 1;
			ULONG ProcessImagesHotPatched : 1;
			ULONG ReservedBits0 : 24;
		};
	};
	union {
		PVOID KernelCallbackTable;
		PVOID UserSharedInfoPtr;
	};
	ULONG SystemReserved;
	ULONG AtlThunkSListPtr32;
	/*PAPI_SET_NAMESPACE*/ PVOID ApiSetMap;
	ULONG TlsExpansionCounter;
	/*PRTL_BITMAP*/ PVOID TlsBitmap;
	ULONG TlsBitmapBits[2];
	PVOID ReadOnlySharedMemoryBase;
	/*PSILO_USER_SHARED_DATA*/ PVOID SharedData;
	PVOID* ReadOnlyStaticServerData;
	PVOID AnsiCodePageData;
	PVOID OemCodePageData;
	PVOID UnicodeCaseTableData;
	ULONG NumberOfProcessors;
	union {
		ULONG NtGlobalFlag;
		struct
		{
			ULONG StopOnException : 1;
			ULONG ShowLoaderSnaps : 1;
			ULONG DebugInitialCommand : 1;
			ULONG StopOnHungGUI : 1;
			ULONG HeapEnableTailCheck : 1;
			ULONG HeapEnableFreeCheck : 1;
			ULONG HeapValidateParameters : 1;
			ULONG HeapValidateAll : 1;
			ULONG ApplicationVerifier : 1;
			ULONG MonitorSilentProcessExit : 1;
			ULONG PoolEnableTagging : 1;
			ULONG HeapEnableTagging : 1;
			ULONG UserStackTraceDb : 1;
			ULONG KernelStackTraceDb : 1;
			ULONG MaintainObjectTypeList : 1;
			ULONG HeapEnableTagByDll : 1;
			ULONG DisableStackExtension : 1;
			ULONG EnableCsrDebug : 1;
			ULONG EnableKDebugSymbolLoad : 1;
			ULONG DisablePageKernelStacks : 1;
			ULONG EnableSystemCritBreaks : 1;
			ULONG HeapDisableCoalescing : 1;
			ULONG EnableCloseExceptions : 1;
			ULONG EnableExceptionLogging : 1;
			ULONG EnableHandleTypeTagging : 1;
			ULONG HeapPageAllocs : 1;
			ULONG DebugInitialCommandEx : 1;
			ULONG DisableDbgPrint : 1;
			ULONG CritSecEventCreation : 1;
			ULONG LdrTopDown : 1;
			ULONG EnableHandleExceptions : 1;
			ULONG DisableProtDlls : 1;
		} NtGlobalFlags;
	};
	LARGE_INTEGER CriticalSectionTimeout;
	SIZE_T HeapSegmentReserve;
	SIZE_T HeapSegmentCommit;
	SIZE_T HeapDeCommitTotalFreeThreshold;
	SIZE_T HeapDeCommitFreeBlockThreshold;
	ULONG NumberOfHeaps;
	ULONG MaximumNumberOfHeaps;
	PVOID* ProcessHeaps;
	PVOID GdiSharedHandleTable;
	PVOID ProcessStarterHelper;
	ULONG GdiDCAttributeList;
	PRTL_CRITICAL_SECTION LoaderLock;
	ULONG OSMajorVersion;
	ULONG OSMinorVersion;
	USHORT OSBuildNumber;
	USHORT OSCSDVersion;
	ULONG OSPlatformId;
	ULONG ImageSubsystem;
	ULONG ImageSubsystemMajorVersion;
	ULONG ImageSubsystemMinorVersion;
	KAFFINITY ActiveProcessAffinityMask;
} _PEB, *_PPEB;

struct ModuleExportLookupData
{
	const char* moduleName;
	size_t moduleNameLength;
	const char** functionNames;
	void*** functionTargets;
	size_t functionsLeft;
};

#if defined(_M_X64)
#define VOLATILE_X64 volatile
#else
#define VOLATILE_X64
#endif

__declspec(dllexport) void* __stdcall InjectShellcode(void* pParameter, DWORD_PTR apcArgument2, DWORD_PTR apcArgument3)
{
	const DllInject::LOAD_LIBRARY_REMOTE_DATA* pInjData = (const DllInject::LOAD_LIBRARY_REMOTE_DATA*)pParameter;

	bool threadCreatedFromAPC = false;
	bool runningFromAPC = false;
	if ((DWORD_PTR)pInjData & 1)
	{
		pInjData = (const DllInject::LOAD_LIBRARY_REMOTE_DATA*)((DWORD_PTR)pInjData & ~1);
		threadCreatedFromAPC = true;
	}
	else
	{
		runningFromAPC = pInjData->bRunningFromAPC;
	}

	// Get the Process Environment Block.
	// https://github.com/sandboxie-plus/Sandboxie/blob/dbf7ae81cfc50db3598085472e5f143b7653e4a8/Sandboxie/common/my_xeb.h#L433
#if defined(_M_X64)
	_PPEB peb = (_PPEB)__readgsqword(0x60);
#elif defined(_M_IX86)
	_PPEB peb = (_PPEB)__readfsdword(0x30);
#elif defined(_M_ARM64)
	_PPEB peb = *(_PPEB*)(__getReg(18) + 0x60); // TEB in x18
#else
#error "This architecture is currently unsupported"
#endif

	// If there's no loader data, we can't do much.
	if (!peb->Ldr)
	{
		return nullptr;
	}

	////////////////////////////////////////////////////////////////////////////////
	// KERNEL32.DLL
	const char szKernel32Dll[] = {'K', 'E', 'R', 'N', 'E', 'L', '3', '2', '.', 'D', 'L', 'L'};

	// Add volatile to long strings to prevent the compiler from using XMM registers and storing their values in the
	// data section.
	const char szLoadLibraryW[] = {'L', 'o', 'a', 'd', 'L', 'i', 'b', 'r', 'a', 'r', 'y', 'W', '\0'};
	const char szGetProcAddress[] = {'G', 'e', 't', 'P', 'r', 'o', 'c', 'A', 'd', 'd', 'r', 'e', 's', 's', '\0'};
	const char szFreeLibrary[] = {'F', 'r', 'e', 'e', 'L', 'i', 'b', 'r', 'a', 'r', 'y', '\0'};
	const char szVirtualFree[] = {'V', 'i', 'r', 't', 'u', 'a', 'l', 'F', 'r', 'e', 'e', '\0'};
	const char szGetLastError[] = {'G', 'e', 't', 'L', 'a', 's', 't', 'E', 'r', 'r', 'o', 'r', '\0'};
	VOLATILE_X64
	const char szOutputDebugStringA[] = {'O', 'u', 't', 'p', 'u', 't', 'D', 'e', 'b', 'u',
										 'g', 'S', 't', 'r', 'i', 'n', 'g', 'A', '\0'};
	const char szCloseHandle[] = {'C', 'l', 'o', 's', 'e', 'H', 'a', 'n', 'd', 'l', 'e', '\0'};
	VOLATILE_X64
	const char szSetThreadErrorMode[] = {'S', 'e', 't', 'T', 'h', 'r', 'e', 'a', 'd', 'E',
										 'r', 'r', 'o', 'r', 'M', 'o', 'd', 'e', '\0'};
	const char szCreateThread[] = {'C', 'r', 'e', 'a', 't', 'e', 'T', 'h', 'r', 'e', 'a', 'd', '\0'};

	const char* kernel32FunctionNames[] = {
		szLoadLibraryW, szGetProcAddress,
		szFreeLibrary,  szVirtualFree,
		szGetLastError, (const char*)szOutputDebugStringA,
		szCloseHandle,  (const char*)szSetThreadErrorMode,
		szCreateThread,
	};

	decltype(&LoadLibraryW) pLoadLibraryW = nullptr;
	decltype(&GetProcAddress) pGetProcAddress = nullptr;
	decltype(&FreeLibrary) pFreeLibrary = nullptr;
	decltype(&VirtualFree) pVirtualFree = nullptr;
	decltype(&GetLastError) pGetLastError = nullptr;
	decltype(&OutputDebugStringA) pOutputDebugStringA = nullptr;
	decltype(&CloseHandle) pCloseHandle = nullptr;
	decltype(&SetThreadErrorMode) pSetThreadErrorMode = nullptr;
	decltype(&CreateThread) pCreateThread = nullptr;

	void** kernel32FunctionTargets[] = {
		(void**)&pLoadLibraryW, (void**)&pGetProcAddress,     (void**)&pFreeLibrary,
		(void**)&pVirtualFree,  (void**)&pGetLastError,       (void**)&pOutputDebugStringA,
		(void**)&pCloseHandle,  (void**)&pSetThreadErrorMode, (void**)&pCreateThread,
	};

	static_assert(std::size(kernel32FunctionNames) == std::size(kernel32FunctionTargets));

	////////////////////////////////////////////////////////////////////////////////
	// Lookup data
	ModuleExportLookupData lookupData[3] = {
		{
			szKernel32Dll,
			std::size(szKernel32Dll),
			kernel32FunctionNames,
			kernel32FunctionTargets,
			std::size(kernel32FunctionNames),
		},
	};

	////////////////////////////////////////////////////////////////////////////////
	// NTDLL.DLL
	const char szNtdll[] = {'N', 'T', 'D', 'L', 'L', '.', 'D', 'L', 'L'};

	VOLATILE_X64
	const char szNtQueueApcThread[] = {'N', 't', 'Q', 'u', 'e', 'u', 'e', 'A', 'p',
									   'c', 'T', 'h', 'r', 'e', 'a', 'd', '\0'};
	const char szNtAlertThread[] = {'N', 't', 'A', 'l', 'e', 'r', 't', 'T', 'h', 'r', 'e', 'a', 'd', '\0'};

	const char* ntdllFunctionNames[] = {
		(const char*)szNtQueueApcThread,
		szNtAlertThread,
	};

	NTSTATUS(NTAPI * pNtQueueApcThread)(HANDLE, PVOID, PVOID, PVOID, PVOID) = nullptr;
	NTSTATUS(NTAPI * pNtAlertThread)(HANDLE) = nullptr;

	void** ntdllFunctionTargets[] = {
		(void**)&pNtQueueApcThread,
		(void**)&pNtAlertThread,
	};

	static_assert(std::size(ntdllFunctionNames) == std::size(ntdllFunctionTargets));

	// The ntdll functions are only needed for APC re-queueing.
	if (runningFromAPC && peb->ProcessInitializing)
	{
		lookupData[1] = {
			szNtdll, std::size(szNtdll), ntdllFunctionNames, ntdllFunctionTargets, std::size(ntdllFunctionNames),
		};
	}

	// Process the kernels exports for the functions our loader needs.

	bool foundAll = false;

	// Get the first entry of the module list.
	PLIST_ENTRY pleInLoadHead = &peb->Ldr->InLoadOrderModuleList;
	PLIST_ENTRY pleInLoadIter = pleInLoadHead->Flink;
	while (pleInLoadIter != pleInLoadHead)
	{
		PLIST_ENTRY pleInLoadCurrent = pleInLoadIter;

		// Get the next entry.
		pleInLoadIter = pleInLoadIter->Flink;

		PCWSTR BaseDllNameBuffer = ((PLDR_DATA_TABLE_ENTRY)pleInLoadCurrent)->BaseDllName.pBuffer;
		USHORT BaseDllNameLength = ((PLDR_DATA_TABLE_ENTRY)pleInLoadCurrent)->BaseDllName.Length / sizeof(WCHAR);
		ModuleExportLookupData* lookupItem = nullptr;

		for (size_t mod = 0; lookupData[mod].moduleName; mod++)
		{
			auto& item = lookupData[mod];

			if (item.functionsLeft == 0)
				continue;

			if (BaseDllNameLength != item.moduleNameLength)
				continue;

			USHORT i;
			for (i = 0; i < BaseDllNameLength; i++)
			{
				WCHAR c = BaseDllNameBuffer[i];
				if (c >= 'a' && c <= 'z')
					c -= 'a' - 'A';

				if (c != item.moduleName[i])
					break;
			}

			if (i == BaseDllNameLength)
			{
				lookupItem = &item;
				break;
			}
		}

		if (!lookupItem)
			continue;

		// Variables for processing the kernel's export table.
		ULONG_PTR uiBaseAddress;
		ULONG_PTR uiAddressArray;
		ULONG_PTR uiNameArray;
		ULONG_PTR uiExportDir;
		ULONG_PTR uiNameOrdinals;
		DWORD dwNumberOfNames;

		// Get this modules base address.
		uiBaseAddress = (ULONG_PTR)((PLDR_DATA_TABLE_ENTRY)pleInLoadCurrent)->DllBase;

		// Get the VA of the modules NT Header.
		uiExportDir = uiBaseAddress + ((PIMAGE_DOS_HEADER)uiBaseAddress)->e_lfanew;

		// uiNameArray = the address of the modules export directory entry.
		uiNameArray =
			(ULONG_PTR) & ((PIMAGE_NT_HEADERS)uiExportDir)->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_EXPORT];

		// Get the VA of the export directory.
		uiExportDir = (uiBaseAddress + ((PIMAGE_DATA_DIRECTORY)uiNameArray)->VirtualAddress);

		// Get the VA for the array of name pointers.
		uiNameArray = (uiBaseAddress + ((PIMAGE_EXPORT_DIRECTORY)uiExportDir)->AddressOfNames);

		// Get the VA for the array of name ordinals.
		uiNameOrdinals = (uiBaseAddress + ((PIMAGE_EXPORT_DIRECTORY)uiExportDir)->AddressOfNameOrdinals);

		// Get the total number of named exports.
		dwNumberOfNames = ((PIMAGE_EXPORT_DIRECTORY)uiExportDir)->NumberOfNames;

		// Loop while we still have imports to find.
		while (lookupItem->functionsLeft > 0 && dwNumberOfNames > 0)
		{
			PCSTR pFunctionName = (PCSTR)(uiBaseAddress + DEREF_32(uiNameArray));
			void** pTargetAddress = nullptr;

			for (size_t i = 0; i < lookupItem->functionsLeft; i++)
			{
				bool matched = false;
				const char* lookupFunctionName = lookupItem->functionNames[i];
				for (size_t j = 0; lookupFunctionName[j] == pFunctionName[j]; j++)
				{
					if (lookupFunctionName[j] == '\0')
					{
						matched = true;
						break;
					}
				}

				if (matched)
				{
					pTargetAddress = lookupItem->functionTargets[i];

					// Compact the arrays if needed.
					if (i < lookupItem->functionsLeft - 1)
					{
						lookupItem->functionNames[i] = lookupItem->functionNames[lookupItem->functionsLeft - 1];
						lookupItem->functionTargets[i] = lookupItem->functionTargets[lookupItem->functionsLeft - 1];
					}

					// Decrement the counter.
					lookupItem->functionsLeft--;

					break;
				}
			}

			// If we have found a function we want, retrieve its virtual address.
			if (pTargetAddress)
			{
				// Get the VA for the array of addresses.
				uiAddressArray = (uiBaseAddress + ((PIMAGE_EXPORT_DIRECTORY)uiExportDir)->AddressOfFunctions);

				// Use this function's name ordinal as an index into the array of name pointers.
				uiAddressArray += (DEREF_16(uiNameOrdinals) * sizeof(DWORD));

				// Store this function's VA.
				*pTargetAddress = (void*)(uiBaseAddress + DEREF_32(uiAddressArray));
			}

			// Move to the next exported function name in the array.
			uiNameArray += sizeof(DWORD);

			// Move to the next exported function name ordinal in the array.
			uiNameOrdinals += sizeof(WORD);

			// Decrement the counter for the number of names left to process.
			dwNumberOfNames--;
		}

		// Stop searching when we have found all the required functions.
		foundAll = true;

		for (size_t mod = 0; lookupData[mod].moduleName; mod++)
		{
			const auto& item = lookupData[mod];
			if (item.functionsLeft > 0)
			{
				foundAll = false;
				break;
			}
		}

		if (foundAll)
			break;
	}

#ifdef MESSAGE_BOX_TEST
	HMODULE hUser32 = pLoadLibraryW(L"user32.dll");
	decltype(&MessageBoxA) pMessageBoxW = (decltype(&MessageBoxA))pGetProcAddress(hUser32, "MessageBoxA");
	const char szMessage[] = {'H', 'e', 'l', 'l', 'o', ',', ' ', 'w', 'o', 'r', 'l', 'd', '!', '\0'};
	MessageBoxA(nullptr, szMessage, szMessage, MB_OK);
	return nullptr;
#endif

	INT32 nLogVerbosity = pInjData->nLogVerbosity;

	// If we are running from an APC and the process is not yet initialized, retry
	// by re-queueing the APC and exiting. Reference:
	// https://x.com/sixtyvividtails/status/1910374252307534071
	if (runningFromAPC && peb->ProcessInitializing)
	{
		DWORD apcRunCount = (DWORD)apcArgument2 + 1;

		if (pOutputDebugStringA && nLogVerbosity >= 2)
		{
			const char c = (char)apcRunCount - 1 + 'A';
			char szApcRetryMessage[] = {'[', 'W', 'H', ']', ' ', 'A', 'P', 'C', ' ', 'R', 'E', ' ', c, '\n', '\0'};
			pOutputDebugStringA(szApcRetryMessage);
		}

		enum class ApcRequeueError : char
		{
			kNone = 0,
			kNoImports = '1',
			kNtQueueApcThread = '2',
			kNtAlertThread = '3',
			kCreateThread = '4',
		};

		bool queued = false;
		ApcRequeueError error = ApcRequeueError::kNone;
		if (apcRunCount >= 10)
		{
			// Limit the amount of attempts do avoid an infinite loop.
			// https://x.com/m417z/status/1923449532920025390
			// As a fallback, create as a new thread.
			if (pCreateThread && pCloseHandle)
			{
				HANDLE hThread = pCreateThread(nullptr, 0, (LPTHREAD_START_ROUTINE)pInjData->pThreadShellcodeAddress,
											   (void*)((DWORD_PTR)pInjData | 1), 0, nullptr);
				if (hThread)
				{
					pCloseHandle(hThread);
					queued = true;
				}
				else
				{
					error = ApcRequeueError::kCreateThread;
				}
			}
			else
			{
				error = ApcRequeueError::kNoImports;
			}
		}
		else if (pNtQueueApcThread && pNtAlertThread)
		{
			HANDLE hCurrentThread = (HANDLE)(LONG_PTR)-2;
			if (SUCCEEDED(pNtQueueApcThread(hCurrentThread, pInjData->pAPCShellcodeAddress, (void*)pInjData,
											(PVOID)(DWORD_PTR)apcRunCount, nullptr)))
			{
				queued = true;
				if (FAILED(pNtAlertThread(hCurrentThread)))
				{
					error = ApcRequeueError::kNtAlertThread;
				}
			}
			else
			{
				error = ApcRequeueError::kNtQueueApcThread;
			}
		}
		else
		{
			error = ApcRequeueError::kNoImports;
		}

		if (error != ApcRequeueError::kNone && pOutputDebugStringA && nLogVerbosity >= 1)
		{
			const char c = (char)error;
			char szApcErrorMessage[] = {'[', 'W', 'H', ']', ' ', 'A', 'P', 'C', ' ', 'E', 'R', 'R', c, '\n', '\0'};
			pOutputDebugStringA(szApcErrorMessage);
		}

		return queued ? nullptr : pVirtualFree;
	}

	if (!foundAll)
	{
		// If possible, at least log the error.
		if (pOutputDebugStringA && nLogVerbosity >= 1)
		{
			char szExportResolutionErrorMessage[] = {'[', 'W', 'H', ']', ' ', 'E', 'X', 'P', '\n', '\0'};
			pOutputDebugStringA(szExportResolutionErrorMessage);
		}

		// If we are running from an APC-created thread and the process is not yet initialized,
		// don't free the shellcode as it might be still executing in the APC.
		return (threadCreatedFromAPC && peb->ProcessInitializing) ? nullptr : pVirtualFree;
	}

	HMODULE hModule;
	const char szInjectInit[] = {'I', 'n', 'j', 'e', 'c', 't', 'I', 'n', 'i', 't', '\0'};
	void* pInjectInit;
	BOOL bInitAttempted = FALSE;
	BOOL bInitSucceeded = FALSE;
	DWORD dwLastErrorValue = 0;
	DWORD dwOldMode;

	// Prevent the system from displaying the critical-error-handler message box.
	// A message box like this was appearing while trying to load a dll in a
	// process with the ProcessSignaturePolicy mitigation, and it looked like this:
	// https://stackoverflow.com/q/38367847
	pSetThreadErrorMode(SEM_FAILCRITICALERRORS, &dwOldMode);

	if (nLogVerbosity >= 2)
	{
		char szLoadLibraryMessage[] = {'[', 'W', 'H', ']', ' ', 'L', 'L', '\n', '\0'};
		pOutputDebugStringA(szLoadLibraryMessage);
	}

	hModule = pLoadLibraryW(pInjData->szDllName);
	if (hModule)
	{
		if (nLogVerbosity >= 2)
		{
			char szGetProcAddressMessage[] = {'[', 'W', 'H', ']', ' ', 'G', 'P', 'A', '\n', '\0'};
			pOutputDebugStringA(szGetProcAddressMessage);
		}

		pInjectInit = pGetProcAddress(hModule, szInjectInit);
		if (pInjectInit)
		{
			if (nLogVerbosity >= 2)
			{
				char szInjectInitMessage[] = {'[', 'W', 'H', ']', ' ', 'I', 'I', '\n', '\0'};
				pOutputDebugStringA(szInjectInitMessage);
			}

			bInitAttempted = TRUE;
			bInitSucceeded = ((BOOL(*)(const DllInject::LOAD_LIBRARY_REMOTE_DATA*))pInjectInit)(pInjData);

			if (nLogVerbosity >= 2)
			{
				char szInjectInitResultMessage[] = {
					'[', 'W', 'H', ']', ' ', 'I', 'I', ':', ' ', bInitSucceeded ? '1' : '0', '\n', '\0'};
				pOutputDebugStringA(szInjectInitResultMessage);
			}
		}
		else
		{
			dwLastErrorValue = pGetLastError();
		}

		pFreeLibrary(hModule);
	}
	else
	{
		dwLastErrorValue = pGetLastError();
	}

	if (!bInitSucceeded)
	{
		if (pInjData->hSessionMutex)
		{
			pCloseHandle(pInjData->hSessionMutex);
		}

		pCloseHandle(pInjData->hSessionManagerProcess);

		if (!bInitAttempted && nLogVerbosity >= 1)
		{
			char szLastErrorMessage[] = {'[', 'W', 'H', ']', ' ', 'E', 'R', 'R', ':',  ' ',
										 '1', '1', '1', '1', '1', '1', '1', '1', '\n', '\0'};
			char* pHex = szLastErrorMessage + sizeof(szLastErrorMessage) - 2;

			for (int i = 0; i < 8; i++)
			{
				int digit = dwLastErrorValue & 0x0F;
				char letter;
				if (digit < 0x0A)
				{
					letter = digit + '0';
				}
				else
				{
					letter = digit - 0x0A + 'A';
				}

				pHex--;
				*pHex = letter;

				dwLastErrorValue >>= 4;
			}

			pOutputDebugStringA(szLastErrorMessage);
		}
	}

	pSetThreadErrorMode(dwOldMode, nullptr);

	// If we are running from an APC-created thread and the process is not yet initialized,
	// don't free the shellcode as it might be still executing in the APC.
	return (threadCreatedFromAPC && peb->ProcessInitializing) ? nullptr : pVirtualFree;
}

// Helpers for creating PRE_ARM64SHELLCODE_VIRTUAL_FREE.
#if 0
using VirtualFree_t = decltype(&VirtualFree);

__declspec(dllexport) __declspec(noinline) VirtualFree_t GetVirtualFree();

__declspec(dllexport) __declspec(noinline) void func1()
{
	VirtualFree_t pVirtualFree = GetVirtualFree();
	if (pVirtualFree)
	{
		pVirtualFree(func1, 0, MEM_RELEASE);
	}
}

__declspec(dllexport) __declspec(noinline) VirtualFree_t GetVirtualFree()
{
	return VirtualFree;
}
#endif

int CALLBACK wWinMain(_In_ HINSTANCE hInstance, _In_opt_ HINSTANCE hPrevInstance, _In_ LPWSTR lpCmdLine,
					  _In_ int nCmdShow)
{
	DllInject::LOAD_LIBRARY_REMOTE_DATA injData{};
	InjectShellcode(&injData, 0, 0);
	return 0;
}
