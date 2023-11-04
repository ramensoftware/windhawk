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
	// LIST_ENTRY InLoadOrderLinks; // As we search from PPEB_LDR_DATA->InMemoryOrderModuleList we dont use first entry.
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
// WinDbg> dt -v ntdll!_PEB
typedef struct __PEB // 65 elements, 0x210 bytes
{
	BYTE bInheritedAddressSpace;
	BYTE bReadImageFileExecOptions;
	BYTE bBeingDebugged;
	BYTE bSpareBool;
	LPVOID lpMutant;
	LPVOID lpImageBaseAddress;
	PPEB_LDR_DATA pLdr;
	LPVOID lpProcessParameters;
	LPVOID lpSubSystemData;
	LPVOID lpProcessHeap;
	PRTL_CRITICAL_SECTION pFastPebLock;
	LPVOID lpFastPebLockRoutine;
	LPVOID lpFastPebUnlockRoutine;
	DWORD dwEnvironmentUpdateCount;
	LPVOID lpKernelCallbackTable;
	DWORD dwSystemReserved;
	DWORD dwAtlThunkSListPtr32;
	PPEB_FREE_BLOCK pFreeList;
	DWORD dwTlsExpansionCounter;
	LPVOID lpTlsBitmap;
	DWORD dwTlsBitmapBits[2];
	LPVOID lpReadOnlySharedMemoryBase;
	LPVOID lpReadOnlySharedMemoryHeap;
	LPVOID lpReadOnlyStaticServerData;
	LPVOID lpAnsiCodePageData;
	LPVOID lpOemCodePageData;
	LPVOID lpUnicodeCaseTableData;
	DWORD dwNumberOfProcessors;
	DWORD dwNtGlobalFlag;
	LARGE_INTEGER liCriticalSectionTimeout;
	DWORD dwHeapSegmentReserve;
	DWORD dwHeapSegmentCommit;
	DWORD dwHeapDeCommitTotalFreeThreshold;
	DWORD dwHeapDeCommitFreeBlockThreshold;
	DWORD dwNumberOfHeaps;
	DWORD dwMaximumNumberOfHeaps;
	LPVOID lpProcessHeaps;
	LPVOID lpGdiSharedHandleTable;
	LPVOID lpProcessStarterHelper;
	DWORD dwGdiDCAttributeList;
	LPVOID lpLoaderLock;
	DWORD dwOSMajorVersion;
	DWORD dwOSMinorVersion;
	WORD wOSBuildNumber;
	WORD wOSCSDVersion;
	DWORD dwOSPlatformId;
	DWORD dwImageSubsystem;
	DWORD dwImageSubsystemMajorVersion;
	DWORD dwImageSubsystemMinorVersion;
	DWORD dwImageProcessAffinityMask;
	DWORD dwGdiHandleBuffer[34];
	LPVOID lpPostProcessInitRoutine;
	LPVOID lpTlsExpansionBitmap;
	DWORD dwTlsExpansionBitmapBits[32];
	DWORD dwSessionId;
	ULARGE_INTEGER liAppCompatFlags;
	ULARGE_INTEGER liAppCompatFlagsUser;
	LPVOID lppShimData;
	LPVOID lpAppCompatInfo;
	UNICODE_STR usCSDVersion;
	LPVOID lpActivationContextData;
	LPVOID lpProcessAssemblyStorageMap;
	LPVOID lpSystemDefaultActivationContextData;
	LPVOID lpSystemAssemblyStorageMap;
	DWORD dwMinimumStackCommit;
} _PEB, *_PPEB;

struct ModuleExportLookupData
{
	const char* moduleName;
	size_t moduleNameLength;
	const char** functionNames;
	void*** functionTargets;
	size_t functionsLeft;
};

__declspec(dllexport) void* __stdcall InjectShellcode(void* pParameter)
{
	const DllInject::LOAD_LIBRARY_REMOTE_DATA* pInjData = (const DllInject::LOAD_LIBRARY_REMOTE_DATA*)pParameter;

	const char szKernel32Dll[] = {'K', 'E', 'R', 'N', 'E', 'L', '3', '2', '.', 'D', 'L', 'L'};

	// Add volatile to long strings to prevent the compiler from using XMM registers and storing their values in the
	// data section.
	const char szLoadLibraryW[] = {'L', 'o', 'a', 'd', 'L', 'i', 'b', 'r', 'a', 'r', 'y', 'W', '\0'};
	const char szGetProcAddress[] = {'G', 'e', 't', 'P', 'r', 'o', 'c', 'A', 'd', 'd', 'r', 'e', 's', 's', '\0'};
	const char szFreeLibrary[] = {'F', 'r', 'e', 'e', 'L', 'i', 'b', 'r', 'a', 'r', 'y', '\0'};
	const char szVirtualFree[] = {'V', 'i', 'r', 't', 'u', 'a', 'l', 'F', 'r', 'e', 'e', '\0'};
	const char szGetLastError[] = {'G', 'e', 't', 'L', 'a', 's', 't', 'E', 'r', 'r', 'o', 'r', '\0'};
	volatile const char szOutputDebugStringA[] = {'O', 'u', 't', 'p', 'u', 't', 'D', 'e', 'b', 'u',
												  'g', 'S', 't', 'r', 'i', 'n', 'g', 'A', '\0'};
	const char szCloseHandle[] = {'C', 'l', 'o', 's', 'e', 'H', 'a', 'n', 'd', 'l', 'e', '\0'};
	volatile const char szSetThreadErrorMode[] = {'S', 'e', 't', 'T', 'h', 'r', 'e', 'a', 'd', 'E',
												  'r', 'r', 'o', 'r', 'M', 'o', 'd', 'e', '\0'};

	const char* kernel32FunctionNames[] = {
		szLoadLibraryW, szGetProcAddress,
		szFreeLibrary,  szVirtualFree,
		szGetLastError, (const char*)szOutputDebugStringA,
		szCloseHandle,  (const char*)szSetThreadErrorMode,
	};

	decltype(&LoadLibraryW) pLoadLibraryW = nullptr;
	decltype(&GetProcAddress) pGetProcAddress = nullptr;
	decltype(&FreeLibrary) pFreeLibrary = nullptr;
	decltype(&VirtualFree) pVirtualFree = nullptr;
	decltype(&GetLastError) pGetLastError = nullptr;
	decltype(&OutputDebugStringA) pOutputDebugStringA = nullptr;
	decltype(&CloseHandle) pCloseHandle = nullptr;
	decltype(&SetThreadErrorMode) pSetThreadErrorMode = nullptr;

	void** kernel32FunctionTargets[] = {
		(void**)&pLoadLibraryW, (void**)&pGetProcAddress,     (void**)&pFreeLibrary, (void**)&pVirtualFree,
		(void**)&pGetLastError, (void**)&pOutputDebugStringA, (void**)&pCloseHandle, (void**)&pSetThreadErrorMode,
	};

	static_assert(std::size(kernel32FunctionNames) == std::size(kernel32FunctionTargets));

	ModuleExportLookupData lookupData[] = {
		{
			szKernel32Dll,
			std::size(szKernel32Dll),
			kernel32FunctionNames,
			kernel32FunctionTargets,
			std::size(kernel32FunctionNames),
		},
	};

	// the kernels base address and later this images newly loaded base address
	ULONG_PTR uiBaseAddress;

	// STEP 1: process the kernels exports for the functions our loader needs...

	// get the Process Environment Block
#ifdef _WIN64
	uiBaseAddress = __readgsqword(0x60);
#else
	uiBaseAddress = __readfsdword(0x30);
#endif

	// get the processes loaded modules. ref: http://msdn.microsoft.com/en-us/library/aa813708(VS.85).aspx
	uiBaseAddress = (ULONG_PTR)((_PPEB)uiBaseAddress)->pLdr;

	// variables for loading this image
	PLIST_ENTRY pleInLoadHead;
	PLIST_ENTRY pleInLoadIter;

	bool foundAll = false;

	// get the first entry of the InMemoryOrder module list
	pleInLoadHead = &((PPEB_LDR_DATA)uiBaseAddress)->InMemoryOrderModuleList;
	pleInLoadIter = pleInLoadHead->Flink;
	while (pleInLoadIter != pleInLoadHead)
	{
		PLIST_ENTRY pleInLoadCurrent = pleInLoadIter;

		// get the next entry
		pleInLoadIter = pleInLoadIter->Flink;

		PCWSTR BaseDllNameBuffer = ((PLDR_DATA_TABLE_ENTRY)pleInLoadCurrent)->BaseDllName.pBuffer;
		USHORT BaseDllNameLength = ((PLDR_DATA_TABLE_ENTRY)pleInLoadCurrent)->BaseDllName.Length / sizeof(WCHAR);
		ModuleExportLookupData* lookupItem = nullptr;

		for (auto& item : lookupData)
		{
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

		// variables for processing the kernels export table
		ULONG_PTR uiAddressArray;
		ULONG_PTR uiNameArray;
		ULONG_PTR uiExportDir;
		ULONG_PTR uiNameOrdinals;
		DWORD dwNumberOfNames;

		// get this modules base address
		uiBaseAddress = (ULONG_PTR)((PLDR_DATA_TABLE_ENTRY)pleInLoadCurrent)->DllBase;

		// get the VA of the modules NT Header
		uiExportDir = uiBaseAddress + ((PIMAGE_DOS_HEADER)uiBaseAddress)->e_lfanew;

		// uiNameArray = the address of the modules export directory entry
		uiNameArray =
			(ULONG_PTR) & ((PIMAGE_NT_HEADERS)uiExportDir)->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_EXPORT];

		// get the VA of the export directory
		uiExportDir = (uiBaseAddress + ((PIMAGE_DATA_DIRECTORY)uiNameArray)->VirtualAddress);

		// get the VA for the array of name pointers
		uiNameArray = (uiBaseAddress + ((PIMAGE_EXPORT_DIRECTORY)uiExportDir)->AddressOfNames);

		// get the VA for the array of name ordinals
		uiNameOrdinals = (uiBaseAddress + ((PIMAGE_EXPORT_DIRECTORY)uiExportDir)->AddressOfNameOrdinals);

		// get total number of named exports
		dwNumberOfNames = ((PIMAGE_EXPORT_DIRECTORY)uiExportDir)->NumberOfNames;

		// loop while we still have imports to find
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

					// compact the arrays if needed
					if (i < lookupItem->functionsLeft - 1)
					{
						lookupItem->functionNames[i] = lookupItem->functionNames[lookupItem->functionsLeft - 1];
						lookupItem->functionTargets[i] = lookupItem->functionTargets[lookupItem->functionsLeft - 1];
					}

					// decrement our counter
					lookupItem->functionsLeft--;

					break;
				}
			}

			// if we have found a function we want we get its virtual address
			if (pTargetAddress)
			{
				// get the VA for the array of addresses
				uiAddressArray = (uiBaseAddress + ((PIMAGE_EXPORT_DIRECTORY)uiExportDir)->AddressOfFunctions);

				// use this functions name ordinal as an index into the array of name pointers
				uiAddressArray += (DEREF_16(uiNameOrdinals) * sizeof(DWORD));

				// store this functions VA
				*pTargetAddress = (void*)(uiBaseAddress + DEREF_32(uiAddressArray));
			}

			// get the next exported function name
			uiNameArray += sizeof(DWORD);

			// get the next exported function name ordinal
			uiNameOrdinals += sizeof(WORD);

			// decrement our # of names counter
			dwNumberOfNames--;
		}

		// we stop searching when we have found everything we need
		foundAll = true;

		for (auto& item : lookupData)
		{
			if (item.functionsLeft > 0)
			{
				foundAll = false;
				break;
			}
		}

		if (foundAll)
			break;
	}

	INT32 nLogVerbosity = pInjData->nLogVerbosity;

	if (!foundAll)
	{
		// If possible, at least log the error.
		if (pOutputDebugStringA && nLogVerbosity >= 1)
		{
			char szExportResolutionErrorMessage[] = {'[', 'W', 'H', ']', ' ', 'E', 'X', 'P', '\n', '\0'};
			pOutputDebugStringA(szExportResolutionErrorMessage);
		}

		return pVirtualFree;
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

	return pVirtualFree;
}

int CALLBACK wWinMain(_In_ HINSTANCE hInstance, _In_opt_ HINSTANCE hPrevInstance, _In_ LPWSTR lpCmdLine,
					  _In_ int nCmdShow)
{
	InjectShellcode((void*)sizeof(DllInject::LOAD_LIBRARY_REMOTE_DATA));
	return 0;
}
