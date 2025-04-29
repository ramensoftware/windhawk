/*
 * KNSoft.SlimDetours (https://github.com/KNSoft/KNSoft.SlimDetours) Instruction Utility
 * Copyright (c) KNSoft.org (https://github.com/KNSoft). All rights reserved.
 * Licensed under the MIT license.
 *
 * Source base on Microsoft Detours:
 *
 * Microsoft Research Detours Package, Version 4.0.1
 * Copyright (c) Microsoft Corporation. All rights reserved.
 * Licensed under the MIT license.
 */

#include "SlimDetours.inl"

static
BOOL
detour_is_imported(
    _In_ PVOID pbCode,
    _In_ PVOID pbAddress)
{
    NTSTATUS Status;
    MEMORY_BASIC_INFORMATION mbi;
    PIMAGE_DOS_HEADER pDosHeader;
    PIMAGE_NT_HEADERS pNtHeader;
    PVOID pEndOfMem;
    WORD wNtMagic;

    Status = NtQueryVirtualMemory(NtCurrentProcess(), pbCode, MemoryBasicInformation, &mbi, sizeof(mbi), NULL);
    if (!NT_SUCCESS(Status))
    {
        return FALSE;
    }

    /* Type should be MEM_IMAGE */
    if (mbi.Type != MEM_IMAGE)
    {
        return FALSE;
    }

    /* Cannot be uncommitted regions or guard pages */
    if ((mbi.State != MEM_COMMIT) || ((mbi.Protect & 0xFF) == PAGE_NOACCESS) || (mbi.Protect & PAGE_GUARD))
    {
        return FALSE;
    }

    /*
     * RegionSize should >= PAGE_SIZE and PAGE_SIZE always >= sizeof(IMAGE_DOS_HEADER),
     * so we can access IMAGE_DOS_HEADER safely without boundary check.
     */
    _STATIC_ASSERT(PAGE_SIZE >= sizeof(IMAGE_DOS_HEADER));
    if (mbi.RegionSize < PAGE_SIZE)
    {
        return FALSE;
    }

    /* Check IMAGE_DOS_HEADER */
    pDosHeader = (PIMAGE_DOS_HEADER)mbi.AllocationBase;
    if (pDosHeader->e_magic != IMAGE_DOS_SIGNATURE)
    {
        return FALSE;
    }
    if (pDosHeader->e_lfanew < sizeof(*pDosHeader) || (ULONG)pDosHeader->e_lfanew > mbi.RegionSize)
    {
        return FALSE;
    }

    /* Now we need perform boundary check in every single step */
    pEndOfMem = Add2Ptr(mbi.AllocationBase, mbi.RegionSize);

    /*
     * Step forward to IMAGE_NT_HEADERS and check IMAGE_NT_SIGNATURE,
     * check FileHeader.SizeOfOptionalHeader == 0 seems pointless
     * unless compare it with sizeof(IMAGE_OPTIONAL_HEADER) explicitly.
     */
    pNtHeader = (PIMAGE_NT_HEADERS)Add2Ptr(pDosHeader, pDosHeader->e_lfanew);
    if (Add2Ptr(pNtHeader, sizeof(*pNtHeader)) > pEndOfMem)
    {
        return FALSE;
    }
    if (pNtHeader->Signature != IMAGE_NT_SIGNATURE)
    {
        return FALSE;
    }

    /* Step forward to IMAGE_OPTIONAL_HEADER and check magic */
    _STATIC_ASSERT(UFIELD_OFFSET(IMAGE_OPTIONAL_HEADER, Magic) == 0);
    wNtMagic = pNtHeader->OptionalHeader.Magic;
    if (wNtMagic != IMAGE_NT_OPTIONAL_HDR_MAGIC ||
        pNtHeader->FileHeader.SizeOfOptionalHeader != sizeof(IMAGE_OPTIONAL_HEADER))
    {
        return FALSE;
    }

    if (pNtHeader->OptionalHeader.NumberOfRvaAndSizes <= IMAGE_DIRECTORY_ENTRY_IAT ||
        pbAddress < Add2Ptr(mbi.AllocationBase,
                            pNtHeader->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IAT].VirtualAddress) ||
        pbAddress >= Add2Ptr(mbi.AllocationBase,
                             pNtHeader->OptionalHeader
                             .DataDirectory[IMAGE_DIRECTORY_ENTRY_IAT].VirtualAddress +
                             pNtHeader->OptionalHeader
                             .DataDirectory[IMAGE_DIRECTORY_ENTRY_IAT].Size))
    {
        return FALSE;
    }

    return TRUE;
}

#if defined(_X86_) || defined(_AMD64_)

_Ret_notnull_
PBYTE
detour_gen_jmp_immediate(
    _In_ PBYTE pbCode,
    _In_ PBYTE pbJmpVal)
{
    PBYTE pbJmpSrc = pbCode + 5;
    *pbCode++ = 0xe9;   // jmp +imm32
    *((INT32*)pbCode) = (INT32)(pbJmpVal - pbJmpSrc);
    return pbCode + sizeof(INT32);
}

BOOL
detour_is_jmp_immediate_to(
    _In_ PBYTE pbCode,
    _In_ PBYTE pbJmpVal)
{
    PBYTE pbJmpSrc = pbCode + 5;
    if (*pbCode++ != 0xe9)   // jmp +imm32
    {
        return FALSE;
    }
    INT32 offset = *((INT32*)pbCode);
    return offset == (INT32)(pbJmpVal - pbJmpSrc);
}

_Ret_notnull_
PBYTE
detour_gen_jmp_indirect(
    _In_ PBYTE pbCode,
    _In_ PBYTE* ppbJmpVal)
{
#if defined(_AMD64_)
    PBYTE pbJmpSrc = pbCode + 6;
#endif
    *pbCode++ = 0xff;   // jmp [+imm32]
    *pbCode++ = 0x25;
#if defined(_AMD64_)
    *((INT32*)pbCode) = (INT32)((PBYTE)ppbJmpVal - pbJmpSrc);
#else
    *((INT32*)pbCode) = (INT32)((PBYTE)ppbJmpVal);
#endif
    return pbCode + sizeof(INT32);
}

BOOL
detour_is_jmp_indirect_to(
    _In_ PBYTE pbCode,
    _In_ PBYTE* ppbJmpVal)
{
#if defined(_AMD64_)
    PBYTE pbJmpSrc = pbCode + 6;
#endif
    if (*pbCode++ != 0xff)   // jmp [+imm32]
    {
        return FALSE;
    }
    if (*pbCode++ != 0x25)
    {
        return FALSE;
    }
    INT32 offset = *((INT32*)pbCode);
#if defined(_AMD64_)
    return offset == (INT32)((PBYTE)ppbJmpVal - pbJmpSrc);
#else
    return offset == (INT32)((PBYTE)ppbJmpVal);
#endif
}

_Ret_notnull_
PBYTE
detour_gen_brk(
    _In_ PBYTE pbCode,
    _In_ PBYTE pbLimit)
{
    while (pbCode < pbLimit)
    {
        *pbCode++ = 0xcc;   // brk;
    }
    return pbCode;
}

_Ret_notnull_
PBYTE
detour_skip_jmp(
    _In_ PBYTE pbCode)
{
    PBYTE pbCodeOriginal;

    // First, skip over the import vector if there is one.
    if (pbCode[0] == 0xff && pbCode[1] == 0x25)
    {
        // Looks like an import alias jump, then get the code it points to.
#if defined(_X86_)
        // jmp [imm32]
        PBYTE pbTarget = *(UNALIGNED PBYTE*) & pbCode[2];
#else
        // jmp [+imm32]
        PBYTE pbTarget = pbCode + 6 + *(UNALIGNED INT32*) & pbCode[2];
#endif

        if (detour_is_imported(pbCode, pbTarget))
        {
            PBYTE pbNew = *(UNALIGNED PBYTE*)pbTarget;
            DETOUR_TRACE("%p->%p: skipped over import table.\n", pbCode, pbNew);
            pbCode = pbNew;
        }
    }

    // Then, skip over a patch jump
    if (pbCode[0] == 0xeb)
    {
        // jmp +imm8
        PBYTE pbNew = pbCode + 2 + *(CHAR*)&pbCode[1];
        DETOUR_TRACE("%p->%p: skipped over short jump.\n", pbCode, pbNew);
        pbCode = pbNew;
        pbCodeOriginal = pbCode;

        // First, skip over the import vector if there is one.
        if (pbCode[0] == 0xff && pbCode[1] == 0x25)
        {
            // Looks like an import alias jump, then get the code it points to.
#if defined(_X86_)
            // jmp [imm32]
            PBYTE pbTarget = *(UNALIGNED PBYTE*) & pbCode[2];
#else
            // jmp [+imm32]
            PBYTE pbTarget = pbCode + 6 + *(UNALIGNED INT32*) & pbCode[2];
#endif
            if (detour_is_imported(pbCode, pbTarget))
            {
                pbNew = *(UNALIGNED PBYTE*)pbTarget;
                DETOUR_TRACE("%p->%p: skipped over import table.\n", pbCode, pbNew);
                pbCode = pbNew;
            }
        }
        // Finally, skip over a long jump if it is the target of the patch jump.
        else if (pbCode[0] == 0xe9)
        {
            // jmp +imm32
            pbNew = pbCode + 5 + *(UNALIGNED INT32*) & pbCode[1];
            DETOUR_TRACE("%p->%p: skipped over long jump.\n", pbCode, pbNew);
            pbCode = pbNew;

            // Patches applied by the OS will jump through an HPAT page to get
            // the target function in the patch image. The jump is always performed
            // to the target function found at the current instruction pointer + PAGE_SIZE - 6 (size of jump).
            // If this is an OS patch, we want to detour at the point of the target function in the base image. 
            if (pbCode[0] == 0xff &&
                pbCode[1] == 0x25 &&
#if defined(_X86_)
                // Ideally, we would detour at the target function, but
                // since it's patched it begins with a short jump (to padding) which isn't long
                // enough to hold the detour code bytes.
                *(UNALIGNED INT32*)&pbCode[2] == (UNALIGNED INT32)(pbCode + PAGE_SIZE))
#else
                // Since we need 5 bytes to perform the jump, detour at the
                // point of the long jump instead of the short jump at the start of the target.
                *(UNALIGNED INT32*)&pbCode[2] == PAGE_SIZE - 6)
#endif
            {
                // jmp [+PAGE_SIZE-6]
                DETOUR_TRACE("%p->%p: OS patch encountered, reset back to long jump 5 bytes prior to target function.\n",
                             pbCode,
                             pbCodeOriginal);
                pbCode = pbCodeOriginal;
            }
        }
    }
    return pbCode;
}

VOID
detour_find_jmp_bounds(
    _In_ PBYTE pbCode,
    _Outptr_ PVOID* ppLower,
    _Outptr_ PVOID* ppUpper)
{
    // We have to place trampolines within +/- 2GB of code.
    PVOID lo = detour_memory_2gb_below(pbCode);
    PVOID hi = detour_memory_2gb_above(pbCode);
    DETOUR_TRACE("[%p..%p..%p]\n", lo, pbCode, hi);

    // And, within +/- 2GB of relative jmp targets.
    if (pbCode[0] == 0xe9)
    {
        // jmp +imm32
        PBYTE pbNew = pbCode + 5 + *(UNALIGNED INT32*) & pbCode[1];

        if (pbNew < pbCode)
        {
            hi = detour_memory_2gb_above(pbNew);
        } else
        {
            lo = detour_memory_2gb_below(pbNew);
        }
        DETOUR_TRACE("[%p..%p..%p] +imm32\n", lo, pbCode, hi);
    }
#if defined(_AMD64_)
    // And, within +/- 2GB of relative jmp vectors.
    else if (pbCode[0] == 0xff && pbCode[1] == 0x25)
    {
        // jmp [+imm32]
        PBYTE pbNew = pbCode + 6 + *(UNALIGNED INT32*) & pbCode[2];

        if (pbNew < pbCode)
        {
            hi = detour_memory_2gb_above(pbNew);
        } else
        {
            lo = detour_memory_2gb_below(pbNew);
        }
        DETOUR_TRACE("[%p..%p..%p] [+imm32]\n", lo, pbCode, hi);
    }
#endif

    *ppLower = lo;
    *ppUpper = hi;
}

BOOL
detour_does_code_end_function(
    _In_ PBYTE pbCode)
{
    if (pbCode[0] == 0xeb ||    // jmp +imm8
        pbCode[0] == 0xe9 ||    // jmp +imm32
        pbCode[0] == 0xe0 ||    // jmp eax
        pbCode[0] == 0xc2 ||    // ret +imm8
        pbCode[0] == 0xc3 ||    // ret
        pbCode[0] == 0xcc)
    {
        // brk
        return TRUE;
    } else if (pbCode[0] == 0xf3 && pbCode[1] == 0xc3)
    {
        // rep ret
        return TRUE;
    } else if (pbCode[0] == 0xff && pbCode[1] == 0x25)
    {
        // jmp [+imm32]
        return TRUE;
    } else if ((pbCode[0] == 0x26 ||      // jmp es:
                pbCode[0] == 0x2e ||      // jmp cs:
                pbCode[0] == 0x36 ||      // jmp ss:
                pbCode[0] == 0x3e ||      // jmp ds:
                pbCode[0] == 0x64 ||      // jmp fs:
                pbCode[0] == 0x65) &&     // jmp gs:
               pbCode[1] == 0xff &&       // jmp [+imm32]
               pbCode[2] == 0x25)
    {
        return TRUE;
    }
    return FALSE;
}

ULONG
detour_is_code_filler(
    _In_ PBYTE pbCode)
{
    // 1-byte through 11-byte NOPs.
    if (pbCode[0] == 0x90)
    {
        return 1;
    }
    if (pbCode[0] == 0x66 && pbCode[1] == 0x90)
    {
        return 2;
    }
    if (pbCode[0] == 0x0F && pbCode[1] == 0x1F && pbCode[2] == 0x00)
    {
        return 3;
    }
    if (pbCode[0] == 0x0F && pbCode[1] == 0x1F && pbCode[2] == 0x40 && pbCode[3] == 0x00)
    {
        return 4;
    }
    if (pbCode[0] == 0x0F && pbCode[1] == 0x1F && pbCode[2] == 0x44 && pbCode[3] == 0x00 && pbCode[4] == 0x00)
    {
        return 5;
    }
    if (pbCode[0] == 0x66 && pbCode[1] == 0x0F && pbCode[2] == 0x1F && pbCode[3] == 0x44 && pbCode[4] == 0x00 &&
        pbCode[5] == 0x00)
    {
        return 6;
    }
    if (pbCode[0] == 0x0F && pbCode[1] == 0x1F && pbCode[2] == 0x80 && pbCode[3] == 0x00 && pbCode[4] == 0x00 &&
        pbCode[5] == 0x00 && pbCode[6] == 0x00)
    {
        return 7;
    }
    if (pbCode[0] == 0x0F && pbCode[1] == 0x1F && pbCode[2] == 0x84 && pbCode[3] == 0x00 && pbCode[4] == 0x00 &&
        pbCode[5] == 0x00 && pbCode[6] == 0x00 && pbCode[7] == 0x00)
    {
        return 8;
    }
    if (pbCode[0] == 0x66 && pbCode[1] == 0x0F && pbCode[2] == 0x1F && pbCode[3] == 0x84 && pbCode[4] == 0x00 &&
        pbCode[5] == 0x00 && pbCode[6] == 0x00 && pbCode[7] == 0x00 && pbCode[8] == 0x00)
    {
        return 9;
    }
    if (pbCode[0] == 0x66 && pbCode[1] == 0x66 && pbCode[2] == 0x0F && pbCode[3] == 0x1F && pbCode[4] == 0x84 &&
        pbCode[5] == 0x00 && pbCode[6] == 0x00 && pbCode[7] == 0x00 && pbCode[8] == 0x00 && pbCode[9] == 0x00)
    {
        return 10;
    }
    if (pbCode[0] == 0x66 && pbCode[1] == 0x66 && pbCode[2] == 0x66 && pbCode[3] == 0x0F && pbCode[4] == 0x1F &&
        pbCode[5] == 0x84 && pbCode[6] == 0x00 && pbCode[7] == 0x00 && pbCode[8] == 0x00 && pbCode[9] == 0x00 &&
        pbCode[10] == 0x00)
    {
        return 11;
    }

    // int 3.
    if (pbCode[0] == 0xCC)
    {
        return 1;
    }
    return 0;
}

#endif // defined(_X86_) || defined(_AMD64_)

#if defined(_ARM64_)
inline
ULONG
fetch_opcode(
    PBYTE pbCode)
{
    return *(ULONG*)pbCode;
}

inline
PBYTE
write_opcode(
    PBYTE pbCode,
    ULONG Opcode)
{
    *(ULONG*)pbCode = Opcode;
    return pbCode + 4;
}

struct ARM64_INDIRECT_JMP
{
    struct
    {
        ULONG Rd : 5;
        ULONG immhi : 19;
        ULONG iop : 5;
        ULONG immlo : 2;
        ULONG op : 1;
    } ardp;

    struct
    {
        ULONG Rt : 5;
        ULONG Rn : 5;
        ULONG imm : 12;
        ULONG opc : 2;
        ULONG iop1 : 2;
        ULONG V : 1;
        ULONG iop2 : 3;
        ULONG size : 2;
    } ldr;

    ULONG br;
};

union ARM64_INDIRECT_IMM
{
    struct
    {
        ULONG64 pad : 12;
        ULONG64 adrp_immlo : 2;
        ULONG64 adrp_immhi : 19;
    };

    LONG64 value;
};

_Ret_notnull_
PBYTE
detour_gen_jmp_indirect(
    _In_ PBYTE pbCode,
    _In_ PULONG64 pbJmpVal)
{
    // adrp x17, [jmpval]
    // ldr x17, [x17, jmpval]
    // br x17

    struct ARM64_INDIRECT_JMP* pIndJmp;
    union ARM64_INDIRECT_IMM jmpIndAddr;

    jmpIndAddr.value = (((LONG64)pbJmpVal) & 0xFFFFFFFFFFFFF000) -
        (((LONG64)pbCode) & 0xFFFFFFFFFFFFF000);

    pIndJmp = (struct ARM64_INDIRECT_JMP*)pbCode;
    pbCode = (PBYTE)(pIndJmp + 1);

    pIndJmp->ardp.Rd = 17;
    pIndJmp->ardp.immhi = (ULONG)jmpIndAddr.adrp_immhi;
    pIndJmp->ardp.iop = 0x10;
    pIndJmp->ardp.immlo = (ULONG)jmpIndAddr.adrp_immlo;
    pIndJmp->ardp.op = 1;

    pIndJmp->ldr.Rt = 17;
    pIndJmp->ldr.Rn = 17;
    pIndJmp->ldr.imm = (((ULONG64)pbJmpVal) & 0xFFF) / 8;
    pIndJmp->ldr.opc = 1;
    pIndJmp->ldr.iop1 = 1;
    pIndJmp->ldr.V = 0;
    pIndJmp->ldr.iop2 = 7;
    pIndJmp->ldr.size = 3;

    pIndJmp->br = 0xD61F0220;

    return pbCode;
}

BOOL
detour_is_jmp_indirect_to(
    _In_ PBYTE pbCode,
    _In_ PULONG64 pbJmpVal)
{
    const struct ARM64_INDIRECT_JMP* pIndJmp;
    union ARM64_INDIRECT_IMM jmpIndAddr;

    jmpIndAddr.value = (((LONG64)pbJmpVal) & 0xFFFFFFFFFFFFF000) -
        (((LONG64)pbCode) & 0xFFFFFFFFFFFFF000);

    pIndJmp = (const struct ARM64_INDIRECT_JMP*)pbCode;

    return pIndJmp->ardp.Rd == 17 &&
        pIndJmp->ardp.immhi == (ULONG)jmpIndAddr.adrp_immhi &&
        pIndJmp->ardp.iop == 0x10 &&
        pIndJmp->ardp.immlo == (ULONG)jmpIndAddr.adrp_immlo &&
        pIndJmp->ardp.op == 1 &&

        pIndJmp->ldr.Rt == 17 &&
        pIndJmp->ldr.Rn == 17 &&
        pIndJmp->ldr.imm == (((ULONG64)pbJmpVal) & 0xFFF) / 8 &&
        pIndJmp->ldr.opc == 1 &&
        pIndJmp->ldr.iop1 == 1 &&
        pIndJmp->ldr.V == 0 &&
        pIndJmp->ldr.iop2 == 7 &&
        pIndJmp->ldr.size == 3 &&

        pIndJmp->br == 0xD61F0220;
}

_Ret_notnull_
PBYTE
detour_gen_jmp_immediate(
    _In_ PBYTE pbCode,
    _In_opt_ PBYTE* ppPool,
    _In_ PBYTE pbJmpVal)
{
    PBYTE pbLiteral;
    if (ppPool != NULL)
    {
        *ppPool = *ppPool - 8;
        pbLiteral = *ppPool;
    } else
    {
        pbLiteral = pbCode + 8;
    }

    *((PBYTE*)pbLiteral) = pbJmpVal;
    LONG delta = (LONG)(pbLiteral - pbCode);

    pbCode = write_opcode(pbCode, 0x58000011 | ((delta / 4) << 5)); // LDR X17,[PC+n]
    pbCode = write_opcode(pbCode, 0xd61f0000 | (17 << 5));          // BR X17

    if (ppPool == NULL)
    {
        pbCode += 8;
    }
    return pbCode;
}

_Ret_notnull_
PBYTE
detour_gen_brk(
    _In_ PBYTE pbCode,
    _In_ PBYTE pbLimit)
{
    while (pbCode < pbLimit)
    {
        pbCode = write_opcode(pbCode, 0xd4100000 | (0xf000 << 5));
    }
    return pbCode;
}

inline
INT64
detour_sign_extend(
    UINT64 value,
    UINT bits)
{
    const UINT left = 64 - bits;
    const INT64 m1 = -1;
    const INT64 wide = (INT64)(value << left);
    const INT64 sign = (wide < 0) ? (m1 << left) : 0;
    return value | sign;
}

_Ret_notnull_
PBYTE
detour_skip_jmp(
    _In_ PBYTE pbCode)
{
    // Skip over the import jump if there is one.
    pbCode = (PBYTE)pbCode;
    ULONG Opcode = fetch_opcode(pbCode);

    if ((Opcode & 0x9f00001f) == 0x90000010)
    {
        // adrp  x16, IAT
        ULONG Opcode2 = fetch_opcode(pbCode + 4);

        if ((Opcode2 & 0xffe003ff) == 0xf9400210)
        {
            // ldr   x16, [x16, IAT]
            ULONG Opcode3 = fetch_opcode(pbCode + 8);

            if (Opcode3 == 0xd61f0200)
            {
                // br    x16

/* https://static.docs.arm.com/ddi0487/bb/DDI0487B_b_armv8_arm.pdf
    The ADRP instruction shifts a signed, 21-bit immediate left by 12 bits, adds it to the value of the program counter with
    the bottom 12 bits cleared to zero, and then writes the result to a general-purpose register. This permits the
    calculation of the address at a 4KB aligned memory region. In conjunction with an ADD (immediate) instruction, or
    a Load/Store instruction with a 12-bit immediate offset, this allows for the calculation of, or access to, any address
    within +/- 4GB of the current PC.

PC-rel. addressing
    This section describes the encoding of the PC-rel. addressing instruction class. The encodings in this section are
    decoded from Data Processing -- Immediate on page C4-226.
    Add/subtract (immediate)
    This section describes the encoding of the Add/subtract (immediate) instruction class. The encodings in this section
    are decoded from Data Processing -- Immediate on page C4-226.
    Decode fields
    Instruction page
    op
    0 ADR
    1 ADRP

C6.2.10 ADRP
    Form PC-relative address to 4KB page adds an immediate value that is shifted left by 12 bits, to the PC value to
    form a PC-relative address, with the bottom 12 bits masked out, and writes the result to the destination register.
    ADRP <Xd>, <label>
    imm = SignExtend(immhi:immlo:Zeros(12), 64);

    31  30 29 28 27 26 25 24 23 5    4 0
    1   immlo  1  0  0  0  0  immhi  Rd
         9             0

Rd is hardcoded as 0x10 above.
Immediate is 21 signed bits split into 2 bits and 19 bits, and is scaled by 4K.
*/
                UINT64 const pageLow2 = (Opcode >> 29) & 3;
                UINT64 const pageHigh19 = (Opcode >> 5) & ~(~0ui64 << 19);
                INT64 const page = detour_sign_extend((pageHigh19 << 2) | pageLow2, 21) << 12;

/* https://static.docs.arm.com/ddi0487/bb/DDI0487B_b_armv8_arm.pdf

    C6.2.101 LDR (immediate)
    Load Register (immediate) loads a word or doubleword from memory and writes it to a register. The address that is
    used for the load is calculated from a base register and an immediate offset.
    The Unsigned offset variant scales the immediate offset value by the size of the value accessed before adding it
    to the base register value.

Unsigned offset
64-bit variant Applies when size == 11.
    31 30 29 28  27 26 25 24  23 22  21   10   9 5   4 0
     1  x  1  1   1  0  0  1   0  1  imm12      Rn    Rt
         F             9        4              200    10

That is, two low 5 bit fields are registers, hardcoded as 0x10 and 0x10 << 5 above,
then unsigned size-unscaled (8) 12-bit offset, then opcode bits 0xF94.
*/
                UINT64 const offset = ((Opcode2 >> 10) & ~(~0ui64 << 12)) << 3;

                PBYTE const pbTarget = (PBYTE)((ULONG64)pbCode & 0xfffffffffffff000ULL) + page + offset;

                if (detour_is_imported(pbCode, pbTarget))
                {
                    PBYTE pbNew = *(PBYTE*)pbTarget;
                    DETOUR_TRACE("%p->%p: skipped over import table.\n", pbCode, pbNew);
                    return pbNew;
                }
            }
        }
    }
    return pbCode;
}

VOID
detour_find_jmp_bounds(
    _In_ PBYTE pbCode,
    _Outptr_ PVOID* ppLower,
    _Outptr_ PVOID* ppUpper)
{
    // The encoding used by detour_gen_jmp_indirect actually enables a
    // displacement of +/- 4GiB. In the future, this could be changed to
    // reflect that. For now, just reuse the x86 logic which is plenty.

    PVOID lo = detour_memory_2gb_below(pbCode);
    PVOID hi = detour_memory_2gb_above(pbCode);
    DETOUR_TRACE("[%p..%p..%p]\n", lo, pbCode, hi);

    *ppLower = lo;
    *ppUpper = hi;
}

static
BOOL
detour_is_code_os_patched(
    _In_ PBYTE pbCode)
{
    // Identify whether the provided code pointer is a OS patch jump.
    // We can do this by checking if a branch (b <imm26>) is present, and if so,
    // it must be jumping to an HPAT page containing ldr <reg> [PC+PAGE_SIZE-4], br <reg>.
    ULONG Opcode = fetch_opcode(pbCode);

    if ((Opcode & 0xfc000000) != 0x14000000)
    {
        return FALSE;
    }
    // The branch must be jumping forward if it's going into the HPAT.
    // Check that the sign bit is cleared.
    if ((Opcode & 0x2000000) != 0)
    {
        return FALSE;
    }
    ULONG Delta = (ULONG)((Opcode & 0x1FFFFFF) * 4);
    PBYTE BranchTarget = pbCode + Delta;

    // Now inspect the opcodes of the code we jumped to in order to determine if it's HPAT.
    ULONG HpatOpcode1 = fetch_opcode(BranchTarget);
    ULONG HpatOpcode2 = fetch_opcode(BranchTarget + 4);

    if (HpatOpcode1 != 0x58008010)
    {
        // ldr <reg> [PC+PAGE_SIZE]
        return FALSE;
    }
    if (HpatOpcode2 != 0xd61f0200)
    {
        // br <reg>
        return FALSE;
    }
    return TRUE;
}

BOOL
detour_does_code_end_function(
    _In_ PBYTE pbCode)
{
    // When the OS has patched a function entry point, it will incorrectly
    // appear as though the function is just a single branch instruction.
    if (detour_is_code_os_patched(pbCode))
    {
        return FALSE;
    }

    ULONG Opcode = fetch_opcode(pbCode);
    if ((Opcode & 0xffbffc1f) == 0xd61f0000 ||  // ret/br <reg>
        (Opcode & 0xfc000000) == 0x14000000)    // b <imm26>
    {
        return TRUE;
    }
    return FALSE;
}

ULONG
detour_is_code_filler(
    _In_ PBYTE pbCode)
{
    if (*(ULONG*)pbCode == 0xd503201f)
    {
        // nop.
        return 4;
    }
    if (*(ULONG*)pbCode == 0x00000000)
    {
        // zero-filled padding.
        return 4;
    }
    return 0;
}

#endif // defined(_ARM64_)

PVOID
NTAPI
SlimDetoursCodeFromPointer(
    _In_ PVOID pPointer)
{
    return detour_skip_jmp((PBYTE)pPointer);
}
