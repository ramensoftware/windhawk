/*
 * KNSoft.SlimDetours (https://github.com/KNSoft/KNSoft.SlimDetours) Disassembler
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

#undef ASSERT
#define ASSERT(x)

//////////////////////////////////////////////////////////////////////////////
//
//  Function:
//      SlimDetoursCopyInstruction(PVOID pDst,
//                                 PVOID *ppDstPool
//                                 PVOID pSrc,
//                                 PVOID *ppTarget,
//                                 LONG *plExtra)
//  Purpose:
//      Copy a single instruction from pSrc to pDst.
//
//  Arguments:
//      pDst:
//          Destination address for the instruction.  May be NULL in which
//          case SlimDetoursCopyInstruction is used to measure an instruction.
//          If not NULL then the source instruction is copied to the
//          destination instruction and any relative arguments are adjusted.
//      ppDstPool:
//          Destination address for the end of the constant pool.  The
//          constant pool works backwards toward pDst.  All memory between
//          pDst and *ppDstPool must be available for use by this function.
//          ppDstPool may be NULL if pDst is NULL.
//      pSrc:
//          Source address of the instruction.
//      ppTarget:
//          Out parameter for any target instruction address pointed to by
//          the instruction.  For example, a branch or a jump insruction has
//          a target, but a load or store instruction doesn't.  A target is
//          another instruction that may be executed as a result of this
//          instruction.  ppTarget may be NULL.
//      plExtra:
//          Out parameter for the number of extra bytes needed by the
//          instruction to reach the target.  For example, lExtra = 3 if the
//          instruction had an 8-bit relative offset, but needs a 32-bit
//          relative offset.
//
//  Returns:
//      Returns the address of the next instruction (following in the source)
//      instruction.  By subtracting pSrc from the return value, the caller
//      can determinte the size of the instruction copied.
//
//  Comments:
//      By following the pTarget, the caller can follow alternate
//      instruction streams.  However, it is not always possible to determine
//      the target based on static analysis.  For example, the destination of
//      a jump relative to a register cannot be determined from just the
//      instruction stream.  The output value, pTarget, can have any of the
//      following outputs:
//          DETOUR_INSTRUCTION_TARGET_NONE:
//              The instruction has no targets.
//          DETOUR_INSTRUCTION_TARGET_DYNAMIC:
//              The instruction has a non-deterministic (dynamic) target.
//              (i.e. the jump is to an address held in a register.)
//          Address:   The instruction has the specified target.
//
//      When copying instructions, SlimDetoursCopyInstruction insures that any
//      targets remain constant.  It does so by adjusting any IP relative
//      offsets.
//

//////////////////////////////////////////////////// X86 and X64 Disassembler.
//
//  Includes full support for all x86 chips prior to the Pentium III, and some newer stuff.
//
#if defined(_AMD64_) || defined(_X86_)

typedef struct _DETOUR_DISASM
{
    BOOL    bOperandOverride;
    BOOL    bAddressOverride;
    BOOL    bRaxOverride; // AMD64 only
    BOOL    bVex;
    BOOL    bEvex;
    BOOL    bF2;
    BOOL    bF3; // x86 only
    BYTE    nSegmentOverride;

    PBYTE*  ppbTarget;
    LONG*   plExtra;

    LONG    lScratchExtra;
    PBYTE   pbScratchTarget;
    BYTE    rbScratchDst[64]; // matches or exceeds rbCode
} DETOUR_DISASM, *PDETOUR_DISASM;

static
VOID
detour_disasm_init(
    _Out_ PDETOUR_DISASM pDisasm,
    _Out_opt_ PBYTE* ppbTarget,
    _Out_opt_ LONG* plExtra)
{
    pDisasm->bOperandOverride = FALSE;
    pDisasm->bAddressOverride = FALSE;
    pDisasm->bRaxOverride = FALSE;
    pDisasm->bF2 = FALSE;
    pDisasm->bF3 = FALSE;
    pDisasm->bVex = FALSE;
    pDisasm->bEvex = FALSE;

    pDisasm->ppbTarget = ppbTarget ? ppbTarget : &pDisasm->pbScratchTarget;
    pDisasm->plExtra = plExtra ? plExtra : &pDisasm->lScratchExtra;

    *pDisasm->ppbTarget = (PBYTE)DETOUR_INSTRUCTION_TARGET_NONE;
    *pDisasm->plExtra = 0;
}

typedef const struct _COPYENTRY *REFCOPYENTRY;

typedef
PBYTE(*COPYFUNC)(
    _In_ PDETOUR_DISASM pDisasm,
    _In_opt_ REFCOPYENTRY pEntry,
    _In_ PBYTE pbDst,
    _In_ PBYTE pbSrc);

// nFlagBits flags.
enum
{
    DYNAMIC = 0x1u,
    ADDRESS = 0x2u,
    NOENLARGE = 0x4u,
    RAX = 0x8u,
};

// ModR/M Flags
enum
{
    SIB = 0x10u,
    RIP = 0x20u,
    NOTSIB = 0x0fu,
};

typedef struct _COPYENTRY
{
    // Many of these fields are often ignored. See ENTRY_DataIgnored.
    ULONG       nFixedSize : 4;     // Fixed size of opcode
    ULONG       nFixedSize16 : 4;   // Fixed size when 16 bit operand
    ULONG       nModOffset : 4;     // Offset to mod/rm byte (0=none)
    ULONG       nRelOffset : 4;     // Offset to relative target.
    ULONG       nFlagBits : 4;      // Flags for DYNAMIC, etc.
    COPYFUNC    pfCopy;             // Function pointer.
} COPYENTRY, *PCOPYENTRY;

static PBYTE CopyBytes(_In_ PDETOUR_DISASM pDisasm, _In_opt_ REFCOPYENTRY pEntry, _In_ PBYTE pbDst, _In_ PBYTE pbSrc);
static PBYTE CopyBytesPrefix(_In_ PDETOUR_DISASM pDisasm, _In_opt_ REFCOPYENTRY pEntry, _In_ PBYTE pbDst, _In_ PBYTE pbSrc);
static PBYTE CopyBytesSegment(_In_ PDETOUR_DISASM pDisasm, _In_opt_ REFCOPYENTRY pEntry, _In_ PBYTE pbDst, _In_ PBYTE pbSrc);
static PBYTE CopyBytesRax(_In_ PDETOUR_DISASM pDisasm, _In_opt_ REFCOPYENTRY pEntry, _In_ PBYTE pbDst, _In_ PBYTE pbSrc);
static PBYTE CopyBytesJump(_In_ PDETOUR_DISASM pDisasm, _In_opt_ REFCOPYENTRY pEntry, _In_ PBYTE pbDst, _In_ PBYTE pbSrc);
static PBYTE Invalid(_In_ PDETOUR_DISASM pDisasm, _In_opt_ REFCOPYENTRY pEntry, _In_ PBYTE pbDst, _In_ PBYTE pbSrc);
static PBYTE Copy0F(_In_ PDETOUR_DISASM pDisasm, _In_opt_ REFCOPYENTRY pEntry, _In_ PBYTE pbDst, _In_ PBYTE pbSrc);
static PBYTE Copy0F78(_In_ PDETOUR_DISASM pDisasm, _In_opt_ REFCOPYENTRY pEntry, _In_ PBYTE pbDst, _In_ PBYTE pbSrc);
static PBYTE Copy0F00(_In_ PDETOUR_DISASM pDisasm, _In_opt_ REFCOPYENTRY pEntry, _In_ PBYTE pbDst, _In_ PBYTE pbSrc);
static PBYTE Copy0FB8(_In_ PDETOUR_DISASM pDisasm, _In_opt_ REFCOPYENTRY pEntry, _In_ PBYTE pbDst, _In_ PBYTE pbSrc);
static PBYTE Copy66(_In_ PDETOUR_DISASM pDisasm, _In_opt_ REFCOPYENTRY pEntry, _In_ PBYTE pbDst, _In_ PBYTE pbSrc);
static PBYTE Copy67(_In_ PDETOUR_DISASM pDisasm, _In_opt_ REFCOPYENTRY pEntry, _In_ PBYTE pbDst, _In_ PBYTE pbSrc);
static PBYTE CopyF2(_In_ PDETOUR_DISASM pDisasm, _In_opt_ REFCOPYENTRY pEntry, _In_ PBYTE pbDst, _In_ PBYTE pbSrc);
static PBYTE CopyF3(_In_ PDETOUR_DISASM pDisasm, _In_opt_ REFCOPYENTRY pEntry, _In_ PBYTE pbDst, _In_ PBYTE pbSrc);
static PBYTE CopyF6(_In_ PDETOUR_DISASM pDisasm, _In_opt_ REFCOPYENTRY pEntry, _In_ PBYTE pbDst, _In_ PBYTE pbSrc);
static PBYTE CopyF7(_In_ PDETOUR_DISASM pDisasm, _In_opt_ REFCOPYENTRY pEntry, _In_ PBYTE pbDst, _In_ PBYTE pbSrc);
static PBYTE CopyFF(_In_ PDETOUR_DISASM pDisasm, _In_opt_ REFCOPYENTRY pEntry, _In_ PBYTE pbDst, _In_ PBYTE pbSrc);
static PBYTE CopyVex3(_In_ PDETOUR_DISASM pDisasm, _In_opt_ REFCOPYENTRY pEntry, _In_ PBYTE pbDst, _In_ PBYTE pbSrc);
static PBYTE CopyVex2(_In_ PDETOUR_DISASM pDisasm, _In_opt_ REFCOPYENTRY pEntry, _In_ PBYTE pbDst, _In_ PBYTE pbSrc);
static PBYTE CopyEvex(_In_ PDETOUR_DISASM pDisasm, _In_opt_ REFCOPYENTRY pEntry, _In_ PBYTE pbDst, _In_ PBYTE pbSrc);
static PBYTE CopyXop(_In_ PDETOUR_DISASM pDisasm, _In_opt_ REFCOPYENTRY pEntry, _In_ PBYTE pbDst, _In_ PBYTE pbSrc);

///////////////////////////////////////////////////////// Disassembler Tables.
//

static const BYTE g_rbModRm[] =
{
    0,0,0,0, SIB | 1,RIP | 4,0,0, 0,0,0,0, SIB | 1,RIP | 4,0,0, // 0x
    0,0,0,0, SIB | 1,RIP | 4,0,0, 0,0,0,0, SIB | 1,RIP | 4,0,0, // 1x
    0,0,0,0, SIB | 1,RIP | 4,0,0, 0,0,0,0, SIB | 1,RIP | 4,0,0, // 2x
    0,0,0,0, SIB | 1,RIP | 4,0,0, 0,0,0,0, SIB | 1,RIP | 4,0,0, // 3x
    1,1,1,1, 2,1,1,1, 1,1,1,1, 2,1,1,1,                         // 4x
    1,1,1,1, 2,1,1,1, 1,1,1,1, 2,1,1,1,                         // 5x
    1,1,1,1, 2,1,1,1, 1,1,1,1, 2,1,1,1,                         // 6x
    1,1,1,1, 2,1,1,1, 1,1,1,1, 2,1,1,1,                         // 7x
    4,4,4,4, 5,4,4,4, 4,4,4,4, 5,4,4,4,                         // 8x
    4,4,4,4, 5,4,4,4, 4,4,4,4, 5,4,4,4,                         // 9x
    4,4,4,4, 5,4,4,4, 4,4,4,4, 5,4,4,4,                         // Ax
    4,4,4,4, 5,4,4,4, 4,4,4,4, 5,4,4,4,                         // Bx
    0,0,0,0, 0,0,0,0, 0,0,0,0, 0,0,0,0,                         // Cx
    0,0,0,0, 0,0,0,0, 0,0,0,0, 0,0,0,0,                         // Dx
    0,0,0,0, 0,0,0,0, 0,0,0,0, 0,0,0,0,                         // Ex
    0,0,0,0, 0,0,0,0, 0,0,0,0, 0,0,0,0                          // Fx
};

enum
{
    eENTRY_CopyBytes1 = 0,
    eENTRY_CopyBytes1Address,
    eENTRY_CopyBytes1Dynamic,
    eENTRY_CopyBytes2,
    eENTRY_CopyBytes2Jump,
    eENTRY_CopyBytes2CantJump,
    eENTRY_CopyBytes2Dynamic,
    eENTRY_CopyBytes3,
    eENTRY_CopyBytes3Dynamic,
    eENTRY_CopyBytes3Or5,
    eENTRY_CopyBytes3Or5Dynamic,
    eENTRY_CopyBytes3Or5Rax,
    eENTRY_CopyBytes3Or5Target,
    eENTRY_CopyBytes4,
    eENTRY_CopyBytes5,
    eENTRY_CopyBytes5Or7Dynamic,
    eENTRY_CopyBytes7,
    eENTRY_CopyBytes2Mod,
    eENTRY_CopyBytes2ModDynamic,
    eENTRY_CopyBytes2Mod1,
    eENTRY_CopyBytes2ModOperand,
    eENTRY_CopyBytes3Mod,
    eENTRY_CopyBytes3Mod1,
    eENTRY_CopyBytesPrefix,
    eENTRY_CopyBytesSegment,
    eENTRY_CopyBytesRax,
    eENTRY_CopyF2,
    eENTRY_CopyF3,
    eENTRY_Copy0F,
    eENTRY_Copy0F78,
    eENTRY_Copy0F00,
    eENTRY_Copy0FB8,
    eENTRY_Copy66,
    eENTRY_Copy67,
    eENTRY_CopyF6,
    eENTRY_CopyF7,
    eENTRY_CopyFF,
    eENTRY_CopyVex2,
    eENTRY_CopyVex3,
    eENTRY_CopyEvex,
    eENTRY_CopyXop,
    eENTRY_CopyBytesXop,
    eENTRY_CopyBytesXop1,
    eENTRY_CopyBytesXop4,
    eENTRY_Invalid
};

// These macros define common uses of nFixedSize, nFixedSize16, nModOffset, nRelOffset, nFlagBits, pfCopy.
#define ENTRY_DataIgnored           0, 0, 0, 0, 0,

static const COPYENTRY g_rceCopyMap[] =
{
    /* eENTRY_CopyBytes1 */            { 1, 1, 0, 0, 0, CopyBytes },
#if defined(_AMD64_)
    /* eENTRY_CopyBytes1Address */     { 9, 5, 0, 0, ADDRESS, CopyBytes },
#else
    /* eENTRY_CopyBytes1Address */     { 5, 3, 0, 0, ADDRESS, CopyBytes },
#endif
    /* eENTRY_CopyBytes1Dynamic */     { 1, 1, 0, 0, DYNAMIC, CopyBytes },
    /* eENTRY_CopyBytes2 */            { 2, 2, 0, 0, 0, CopyBytes },
    /* eENTRY_CopyBytes2Jump */        { ENTRY_DataIgnored CopyBytesJump },
    /* eENTRY_CopyBytes2CantJump */    { 2, 2, 0, 1, NOENLARGE, CopyBytes },
    /* eENTRY_CopyBytes2Dynamic */     { 2, 2, 0, 0, DYNAMIC, CopyBytes },
    /* eENTRY_CopyBytes3 */            { 3, 3, 0, 0, 0, CopyBytes },
    /* eENTRY_CopyBytes3Dynamic */     { 3, 3, 0, 0, DYNAMIC, CopyBytes },
    /* eENTRY_CopyBytes3Or5 */         { 5, 3, 0, 0, 0, CopyBytes },
    /* eENTRY_CopyBytes3Or5Dynamic */  { 5, 3, 0, 0, DYNAMIC, CopyBytes }, // x86 only
#if defined(_AMD64_)
    /* eENTRY_CopyBytes3Or5Rax */      { 5, 3, 0, 0, RAX, CopyBytes },
    /* eENTRY_CopyBytes3Or5Target */   { 5, 5, 0, 1, 0, CopyBytes },
#else
    /* eENTRY_CopyBytes3Or5Rax */      { 5, 3, 0, 0, 0, CopyBytes },
    /* eENTRY_CopyBytes3Or5Target */   { 5, 3, 0, 1, 0, CopyBytes },
#endif
    /* eENTRY_CopyBytes4 */            { 4, 4, 0, 0, 0, CopyBytes },
    /* eENTRY_CopyBytes5 */            { 5, 5, 0, 0, 0, CopyBytes },
    /* eENTRY_CopyBytes5Or7Dynamic */  { 7, 5, 0, 0, DYNAMIC, CopyBytes },
    /* eENTRY_CopyBytes7 */            { 7, 7, 0, 0, 0, CopyBytes },
    /* eENTRY_CopyBytes2Mod */         { 2, 2, 1, 0, 0, CopyBytes },
    /* eENTRY_CopyBytes2ModDynamic */  { 2, 2, 1, 0, DYNAMIC, CopyBytes },
    /* eENTRY_CopyBytes2Mod1 */        { 3, 3, 1, 0, 0, CopyBytes },
    /* eENTRY_CopyBytes2ModOperand */  { 6, 4, 1, 0, 0, CopyBytes },
    /* eENTRY_CopyBytes3Mod */         { 3, 3, 2, 0, 0, CopyBytes }, // SSE3 0F 38 opcode modrm
    /* eENTRY_CopyBytes3Mod1 */        { 4, 4, 2, 0, 0, CopyBytes }, // SSE3 0F 3A opcode modrm .. imm8
    /* eENTRY_CopyBytesPrefix */       { ENTRY_DataIgnored CopyBytesPrefix },
    /* eENTRY_CopyBytesSegment */      { ENTRY_DataIgnored CopyBytesSegment },
    /* eENTRY_CopyBytesRax */          { ENTRY_DataIgnored CopyBytesRax },
    /* eENTRY_CopyF2 */                { ENTRY_DataIgnored CopyF2 },
    /* eENTRY_CopyF3 */                { ENTRY_DataIgnored CopyF3 }, // 32bit x86 only
    /* eENTRY_Copy0F */                { ENTRY_DataIgnored Copy0F },
    /* eENTRY_Copy0F78 */              { ENTRY_DataIgnored Copy0F78 },
    /* eENTRY_Copy0F00 */              { ENTRY_DataIgnored Copy0F00 }, // 32bit x86 only
    /* eENTRY_Copy0FB8 */              { ENTRY_DataIgnored Copy0FB8 }, // 32bit x86 only
    /* eENTRY_Copy66 */                { ENTRY_DataIgnored Copy66 },
    /* eENTRY_Copy67 */                { ENTRY_DataIgnored Copy67 },
    /* eENTRY_CopyF6 */                { ENTRY_DataIgnored CopyF6 },
    /* eENTRY_CopyF7 */                { ENTRY_DataIgnored CopyF7 },
    /* eENTRY_CopyFF */                { ENTRY_DataIgnored CopyFF },
    /* eENTRY_CopyVex2 */              { ENTRY_DataIgnored CopyVex2 },
    /* eENTRY_CopyVex3 */              { ENTRY_DataIgnored CopyVex3 },
    /* eENTRY_CopyEvex */              { ENTRY_DataIgnored CopyEvex }, // 62, 3 byte payload, then normal with implied prefixes like vex
    /* eENTRY_CopyXop */               { ENTRY_DataIgnored CopyXop }, // 0x8F ... POP /0 or AMD XOP
    /* eENTRY_CopyBytesXop */          { 5, 5, 4, 0, 0, CopyBytes }, // 0x8F xop1 xop2 opcode modrm
    /* eENTRY_CopyBytesXop1 */         { 6, 6, 4, 0, 0, CopyBytes }, // 0x8F xop1 xop2 opcode modrm ... imm8
    /* eENTRY_CopyBytesXop4 */         { 9, 9, 4, 0, 0, CopyBytes }, // 0x8F xop1 xop2 opcode modrm ... imm32
    /* eENTRY_Invalid */               { ENTRY_DataIgnored Invalid }
};

static const BYTE g_rceCopyTable[] =
{
    /* 00 */ eENTRY_CopyBytes2Mod,                  // ADD /r
    /* 01 */ eENTRY_CopyBytes2Mod,                  // ADD /r
    /* 02 */ eENTRY_CopyBytes2Mod,                  // ADD /r
    /* 03 */ eENTRY_CopyBytes2Mod,                  // ADD /r
    /* 04 */ eENTRY_CopyBytes2,                     // ADD ib
    /* 05 */ eENTRY_CopyBytes3Or5,                  // ADD iw
#if defined(_AMD64_)
    /* 06 */ eENTRY_Invalid,                        // Invalid
    /* 07 */ eENTRY_Invalid,                        // Invalid
#else
    /* 06 */ eENTRY_CopyBytes1,                     // PUSH
    /* 07 */ eENTRY_CopyBytes1,                     // POP
#endif
    /* 08 */ eENTRY_CopyBytes2Mod,                  // OR /r
    /* 09 */ eENTRY_CopyBytes2Mod,                  // OR /r
    /* 0A */ eENTRY_CopyBytes2Mod,                  // OR /r
    /* 0B */ eENTRY_CopyBytes2Mod,                  // OR /r
    /* 0C */ eENTRY_CopyBytes2,                     // OR ib
    /* 0D */ eENTRY_CopyBytes3Or5,                  // OR iw
#if defined(_AMD64_)
    /* 0E */ eENTRY_Invalid,                        // Invalid
#else
    /* 0E */ eENTRY_CopyBytes1,                     // PUSH
#endif
    /* 0F */ eENTRY_Copy0F,                         // Extension Ops
    /* 10 */ eENTRY_CopyBytes2Mod,                  // ADC /r
    /* 11 */ eENTRY_CopyBytes2Mod,                  // ADC /r
    /* 12 */ eENTRY_CopyBytes2Mod,                  // ADC /r
    /* 13 */ eENTRY_CopyBytes2Mod,                  // ADC /r
    /* 14 */ eENTRY_CopyBytes2,                     // ADC ib
    /* 15 */ eENTRY_CopyBytes3Or5,                  // ADC id
#if defined(_AMD64_)
    /* 16 */ eENTRY_Invalid,                        // Invalid
    /* 17 */ eENTRY_Invalid,                        // Invalid
#else
    /* 16 */ eENTRY_CopyBytes1,                     // PUSH
    /* 17 */ eENTRY_CopyBytes1,                     // POP
#endif
    /* 18 */ eENTRY_CopyBytes2Mod,                  // SBB /r
    /* 19 */ eENTRY_CopyBytes2Mod,                  // SBB /r
    /* 1A */ eENTRY_CopyBytes2Mod,                  // SBB /r
    /* 1B */ eENTRY_CopyBytes2Mod,                  // SBB /r
    /* 1C */ eENTRY_CopyBytes2,                     // SBB ib
    /* 1D */ eENTRY_CopyBytes3Or5,                  // SBB id
#if defined(_AMD64_)
    /* 1E */ eENTRY_Invalid,                        // Invalid
    /* 1F */ eENTRY_Invalid,                        // Invalid
#else
    /* 1E */ eENTRY_CopyBytes1,                     // PUSH
    /* 1F */ eENTRY_CopyBytes1,                     // POP
#endif
    /* 20 */ eENTRY_CopyBytes2Mod,                  // AND /r
    /* 21 */ eENTRY_CopyBytes2Mod,                  // AND /r
    /* 22 */ eENTRY_CopyBytes2Mod,                  // AND /r
    /* 23 */ eENTRY_CopyBytes2Mod,                  // AND /r
    /* 24 */ eENTRY_CopyBytes2,                     // AND ib
    /* 25 */ eENTRY_CopyBytes3Or5,                  // AND id
    /* 26 */ eENTRY_CopyBytesSegment,               // ES prefix
#if defined(_AMD64_)
    /* 27 */ eENTRY_Invalid,                        // Invalid
#else
    /* 27 */ eENTRY_CopyBytes1,                     // DAA
#endif
    /* 28 */ eENTRY_CopyBytes2Mod,                  // SUB /r
    /* 29 */ eENTRY_CopyBytes2Mod,                  // SUB /r
    /* 2A */ eENTRY_CopyBytes2Mod,                  // SUB /r
    /* 2B */ eENTRY_CopyBytes2Mod,                  // SUB /r
    /* 2C */ eENTRY_CopyBytes2,                     // SUB ib
    /* 2D */ eENTRY_CopyBytes3Or5,                  // SUB id
    /* 2E */ eENTRY_CopyBytesSegment,               // CS prefix
#if defined(_AMD64_)
    /* 2F */ eENTRY_Invalid,                        // Invalid
#else
    /* 2F */ eENTRY_CopyBytes1,                     // DAS
#endif
    /* 30 */ eENTRY_CopyBytes2Mod,                  // XOR /r
    /* 31 */ eENTRY_CopyBytes2Mod,                  // XOR /r
    /* 32 */ eENTRY_CopyBytes2Mod,                  // XOR /r
    /* 33 */ eENTRY_CopyBytes2Mod,                  // XOR /r
    /* 34 */ eENTRY_CopyBytes2,                     // XOR ib
    /* 35 */ eENTRY_CopyBytes3Or5,                  // XOR id
    /* 36 */ eENTRY_CopyBytesSegment,               // SS prefix
#if defined(_AMD64_)
    /* 37 */ eENTRY_Invalid,                        // Invalid
#else
    /* 37 */ eENTRY_CopyBytes1,                     // AAA
#endif
    /* 38 */ eENTRY_CopyBytes2Mod,                  // CMP /r
    /* 39 */ eENTRY_CopyBytes2Mod,                  // CMP /r
    /* 3A */ eENTRY_CopyBytes2Mod,                  // CMP /r
    /* 3B */ eENTRY_CopyBytes2Mod,                  // CMP /r
    /* 3C */ eENTRY_CopyBytes2,                     // CMP ib
    /* 3D */ eENTRY_CopyBytes3Or5,                  // CMP id
    /* 3E */ eENTRY_CopyBytesSegment,               // DS prefix
#if defined(_AMD64_)
    /* 3F */ eENTRY_Invalid,                        // Invalid
#else
    /* 3F */ eENTRY_CopyBytes1,                     // AAS
#endif
#if defined(_AMD64_) // For Rax Prefix
    /* 40 */ eENTRY_CopyBytesRax,                   // Rax
    /* 41 */ eENTRY_CopyBytesRax,                   // Rax
    /* 42 */ eENTRY_CopyBytesRax,                   // Rax
    /* 43 */ eENTRY_CopyBytesRax,                   // Rax
    /* 44 */ eENTRY_CopyBytesRax,                   // Rax
    /* 45 */ eENTRY_CopyBytesRax,                   // Rax
    /* 46 */ eENTRY_CopyBytesRax,                   // Rax
    /* 47 */ eENTRY_CopyBytesRax,                   // Rax
    /* 48 */ eENTRY_CopyBytesRax,                   // Rax
    /* 49 */ eENTRY_CopyBytesRax,                   // Rax
    /* 4A */ eENTRY_CopyBytesRax,                   // Rax
    /* 4B */ eENTRY_CopyBytesRax,                   // Rax
    /* 4C */ eENTRY_CopyBytesRax,                   // Rax
    /* 4D */ eENTRY_CopyBytesRax,                   // Rax
    /* 4E */ eENTRY_CopyBytesRax,                   // Rax
    /* 4F */ eENTRY_CopyBytesRax,                   // Rax
#else
    /* 40 */ eENTRY_CopyBytes1,                     // INC
    /* 41 */ eENTRY_CopyBytes1,                     // INC
    /* 42 */ eENTRY_CopyBytes1,                     // INC
    /* 43 */ eENTRY_CopyBytes1,                     // INC
    /* 44 */ eENTRY_CopyBytes1,                     // INC
    /* 45 */ eENTRY_CopyBytes1,                     // INC
    /* 46 */ eENTRY_CopyBytes1,                     // INC
    /* 47 */ eENTRY_CopyBytes1,                     // INC
    /* 48 */ eENTRY_CopyBytes1,                     // DEC
    /* 49 */ eENTRY_CopyBytes1,                     // DEC
    /* 4A */ eENTRY_CopyBytes1,                     // DEC
    /* 4B */ eENTRY_CopyBytes1,                     // DEC
    /* 4C */ eENTRY_CopyBytes1,                     // DEC
    /* 4D */ eENTRY_CopyBytes1,                     // DEC
    /* 4E */ eENTRY_CopyBytes1,                     // DEC
    /* 4F */ eENTRY_CopyBytes1,                     // DEC
#endif
    /* 50 */ eENTRY_CopyBytes1,                     // PUSH
    /* 51 */ eENTRY_CopyBytes1,                     // PUSH
    /* 52 */ eENTRY_CopyBytes1,                     // PUSH
    /* 53 */ eENTRY_CopyBytes1,                     // PUSH
    /* 54 */ eENTRY_CopyBytes1,                     // PUSH
    /* 55 */ eENTRY_CopyBytes1,                     // PUSH
    /* 56 */ eENTRY_CopyBytes1,                     // PUSH
    /* 57 */ eENTRY_CopyBytes1,                     // PUSH
    /* 58 */ eENTRY_CopyBytes1,                     // POP
    /* 59 */ eENTRY_CopyBytes1,                     // POP
    /* 5A */ eENTRY_CopyBytes1,                     // POP
    /* 5B */ eENTRY_CopyBytes1,                     // POP
    /* 5C */ eENTRY_CopyBytes1,                     // POP
    /* 5D */ eENTRY_CopyBytes1,                     // POP
    /* 5E */ eENTRY_CopyBytes1,                     // POP
    /* 5F */ eENTRY_CopyBytes1,                     // POP
#if defined(_AMD64_)
    /* 60 */ eENTRY_Invalid,                        // Invalid
    /* 61 */ eENTRY_Invalid,                        // Invalid
    /* 62 */ eENTRY_CopyEvex,                       // EVEX / AVX512
#else
    /* 60 */ eENTRY_CopyBytes1,                     // PUSHAD
    /* 61 */ eENTRY_CopyBytes1,                     // POPAD
    /* 62 */ eENTRY_CopyEvex,                       // BOUND /r and EVEX / AVX512
#endif
    /* 63 */ eENTRY_CopyBytes2Mod,                  // 32bit ARPL /r, 64bit MOVSXD
    /* 64 */ eENTRY_CopyBytesSegment,               // FS prefix
    /* 65 */ eENTRY_CopyBytesSegment,               // GS prefix
    /* 66 */ eENTRY_Copy66,                         // Operand Prefix
    /* 67 */ eENTRY_Copy67,                         // Address Prefix
    /* 68 */ eENTRY_CopyBytes3Or5,                  // PUSH
    /* 69 */ eENTRY_CopyBytes2ModOperand,           // IMUL /r iz
    /* 6A */ eENTRY_CopyBytes2,                     // PUSH
    /* 6B */ eENTRY_CopyBytes2Mod1,                 // IMUL /r ib
    /* 6C */ eENTRY_CopyBytes1,                     // INS
    /* 6D */ eENTRY_CopyBytes1,                     // INS
    /* 6E */ eENTRY_CopyBytes1,                     // OUTS/OUTSB
    /* 6F */ eENTRY_CopyBytes1,                     // OUTS/OUTSW
    /* 70 */ eENTRY_CopyBytes2Jump,                 // JO           // 0f80
    /* 71 */ eENTRY_CopyBytes2Jump,                 // JNO          // 0f81
    /* 72 */ eENTRY_CopyBytes2Jump,                 // JB/JC/JNAE   // 0f82
    /* 73 */ eENTRY_CopyBytes2Jump,                 // JAE/JNB/JNC  // 0f83
    /* 74 */ eENTRY_CopyBytes2Jump,                 // JE/JZ        // 0f84
    /* 75 */ eENTRY_CopyBytes2Jump,                 // JNE/JNZ      // 0f85
    /* 76 */ eENTRY_CopyBytes2Jump,                 // JBE/JNA      // 0f86
    /* 77 */ eENTRY_CopyBytes2Jump,                 // JA/JNBE      // 0f87
    /* 78 */ eENTRY_CopyBytes2Jump,                 // JS           // 0f88
    /* 79 */ eENTRY_CopyBytes2Jump,                 // JNS          // 0f89
    /* 7A */ eENTRY_CopyBytes2Jump,                 // JP/JPE       // 0f8a
    /* 7B */ eENTRY_CopyBytes2Jump,                 // JNP/JPO      // 0f8b
    /* 7C */ eENTRY_CopyBytes2Jump,                 // JL/JNGE      // 0f8c
    /* 7D */ eENTRY_CopyBytes2Jump,                 // JGE/JNL      // 0f8d
    /* 7E */ eENTRY_CopyBytes2Jump,                 // JLE/JNG      // 0f8e
    /* 7F */ eENTRY_CopyBytes2Jump,                 // JG/JNLE      // 0f8f
    /* 80 */ eENTRY_CopyBytes2Mod1,                 // ADD/0 OR/1 ADC/2 SBB/3 AND/4 SUB/5 XOR/6 CMP/7 byte reg, immediate byte
    /* 81 */ eENTRY_CopyBytes2ModOperand,           // ADD/0 OR/1 ADC/2 SBB/3 AND/4 SUB/5 XOR/6 CMP/7 byte reg, immediate word or dword
#if defined(_AMD64_)
    /* 82 */ eENTRY_Invalid,                        // Invalid
#else
    /* 82 */ eENTRY_CopyBytes2Mod1,                 // MOV al,x
#endif
    /* 83 */ eENTRY_CopyBytes2Mod1,                 // ADD/0 OR/1 ADC/2 SBB/3 AND/4 SUB/5 XOR/6 CMP/7 reg, immediate byte
    /* 84 */ eENTRY_CopyBytes2Mod,                  // TEST /r
    /* 85 */ eENTRY_CopyBytes2Mod,                  // TEST /r
    /* 86 */ eENTRY_CopyBytes2Mod,                  // XCHG /r @todo
    /* 87 */ eENTRY_CopyBytes2Mod,                  // XCHG /r @todo
    /* 88 */ eENTRY_CopyBytes2Mod,                  // MOV /r
    /* 89 */ eENTRY_CopyBytes2Mod,                  // MOV /r
    /* 8A */ eENTRY_CopyBytes2Mod,                  // MOV /r
    /* 8B */ eENTRY_CopyBytes2Mod,                  // MOV /r
    /* 8C */ eENTRY_CopyBytes2Mod,                  // MOV /r
    /* 8D */ eENTRY_CopyBytes2Mod,                  // LEA /r
    /* 8E */ eENTRY_CopyBytes2Mod,                  // MOV /r
    /* 8F */ eENTRY_CopyXop,                        // POP /0 or AMD XOP
    /* 90 */ eENTRY_CopyBytes1,                     // NOP
    /* 91 */ eENTRY_CopyBytes1,                     // XCHG
    /* 92 */ eENTRY_CopyBytes1,                     // XCHG
    /* 93 */ eENTRY_CopyBytes1,                     // XCHG
    /* 94 */ eENTRY_CopyBytes1,                     // XCHG
    /* 95 */ eENTRY_CopyBytes1,                     // XCHG
    /* 96 */ eENTRY_CopyBytes1,                     // XCHG
    /* 97 */ eENTRY_CopyBytes1,                     // XCHG
    /* 98 */ eENTRY_CopyBytes1,                     // CWDE
    /* 99 */ eENTRY_CopyBytes1,                     // CDQ
#if defined(_AMD64_)
    /* 9A */ eENTRY_Invalid,                        // Invalid
#else
    /* 9A */ eENTRY_CopyBytes5Or7Dynamic,           // CALL cp
#endif
    /* 9B */ eENTRY_CopyBytes1,                     // WAIT/FWAIT
    /* 9C */ eENTRY_CopyBytes1,                     // PUSHFD
    /* 9D */ eENTRY_CopyBytes1,                     // POPFD
    /* 9E */ eENTRY_CopyBytes1,                     // SAHF
    /* 9F */ eENTRY_CopyBytes1,                     // LAHF
    /* A0 */ eENTRY_CopyBytes1Address,              // MOV
    /* A1 */ eENTRY_CopyBytes1Address,              // MOV
    /* A2 */ eENTRY_CopyBytes1Address,              // MOV
    /* A3 */ eENTRY_CopyBytes1Address,              // MOV
    /* A4 */ eENTRY_CopyBytes1,                     // MOVS
    /* A5 */ eENTRY_CopyBytes1,                     // MOVS/MOVSD
    /* A6 */ eENTRY_CopyBytes1,                     // CMPS/CMPSB
    /* A7 */ eENTRY_CopyBytes1,                     // CMPS/CMPSW
    /* A8 */ eENTRY_CopyBytes2,                     // TEST
    /* A9 */ eENTRY_CopyBytes3Or5,                  // TEST
    /* AA */ eENTRY_CopyBytes1,                     // STOS/STOSB
    /* AB */ eENTRY_CopyBytes1,                     // STOS/STOSW
    /* AC */ eENTRY_CopyBytes1,                     // LODS/LODSB
    /* AD */ eENTRY_CopyBytes1,                     // LODS/LODSW
    /* AE */ eENTRY_CopyBytes1,                     // SCAS/SCASB
    /* AF */ eENTRY_CopyBytes1,                     // SCAS/SCASD
    /* B0 */ eENTRY_CopyBytes2,                     // MOV B0+rb
    /* B1 */ eENTRY_CopyBytes2,                     // MOV B0+rb
    /* B2 */ eENTRY_CopyBytes2,                     // MOV B0+rb
    /* B3 */ eENTRY_CopyBytes2,                     // MOV B0+rb
    /* B4 */ eENTRY_CopyBytes2,                     // MOV B0+rb
    /* B5 */ eENTRY_CopyBytes2,                     // MOV B0+rb
    /* B6 */ eENTRY_CopyBytes2,                     // MOV B0+rb
    /* B7 */ eENTRY_CopyBytes2,                     // MOV B0+rb
    /* B8 */ eENTRY_CopyBytes3Or5Rax,               // MOV B8+rb
    /* B9 */ eENTRY_CopyBytes3Or5Rax,               // MOV B8+rb
    /* BA */ eENTRY_CopyBytes3Or5Rax,               // MOV B8+rb
    /* BB */ eENTRY_CopyBytes3Or5Rax,               // MOV B8+rb
    /* BC */ eENTRY_CopyBytes3Or5Rax,               // MOV B8+rb
    /* BD */ eENTRY_CopyBytes3Or5Rax,               // MOV B8+rb
    /* BE */ eENTRY_CopyBytes3Or5Rax,               // MOV B8+rb
    /* BF */ eENTRY_CopyBytes3Or5Rax,               // MOV B8+rb
    /* C0 */ eENTRY_CopyBytes2Mod1,                 // RCL/2 ib, etc.
    /* C1 */ eENTRY_CopyBytes2Mod1,                 // RCL/2 ib, etc.
    /* C2 */ eENTRY_CopyBytes3,                     // RET
    /* C3 */ eENTRY_CopyBytes1,                     // RET
    /* C4 */ eENTRY_CopyVex3,                       // LES, VEX 3-byte opcodes.
    /* C5 */ eENTRY_CopyVex2,                       // LDS, VEX 2-byte opcodes.
    /* C6 */ eENTRY_CopyBytes2Mod1,                 // MOV
    /* C7 */ eENTRY_CopyBytes2ModOperand,           // MOV/0 XBEGIN/7
    /* C8 */ eENTRY_CopyBytes4,                     // ENTER
    /* C9 */ eENTRY_CopyBytes1,                     // LEAVE
    /* CA */ eENTRY_CopyBytes3Dynamic,              // RET
    /* CB */ eENTRY_CopyBytes1Dynamic,              // RET
    /* CC */ eENTRY_CopyBytes1Dynamic,              // INT 3
    /* CD */ eENTRY_CopyBytes2Dynamic,              // INT ib
#if defined(_AMD64_)
    /* CE */ eENTRY_Invalid,                        // Invalid
#else
    /* CE */ eENTRY_CopyBytes1Dynamic,              // INTO
#endif
    /* CF */ eENTRY_CopyBytes1Dynamic,              // IRET
    /* D0 */ eENTRY_CopyBytes2Mod,                  // RCL/2, etc.
    /* D1 */ eENTRY_CopyBytes2Mod,                  // RCL/2, etc.
    /* D2 */ eENTRY_CopyBytes2Mod,                  // RCL/2, etc.
    /* D3 */ eENTRY_CopyBytes2Mod,                  // RCL/2, etc.
#if defined(_AMD64_)
    /* D4 */ eENTRY_Invalid,                        // Invalid
    /* D5 */ eENTRY_Invalid,                        // Invalid
#else
    /* D4 */ eENTRY_CopyBytes2,                     // AAM
    /* D5 */ eENTRY_CopyBytes2,                     // AAD
#endif
    /* D6 */ eENTRY_Invalid,                        // Invalid
    /* D7 */ eENTRY_CopyBytes1,                     // XLAT/XLATB
    /* D8 */ eENTRY_CopyBytes2Mod,                  // FADD, etc.
    /* D9 */ eENTRY_CopyBytes2Mod,                  // F2XM1, etc.
    /* DA */ eENTRY_CopyBytes2Mod,                  // FLADD, etc.
    /* DB */ eENTRY_CopyBytes2Mod,                  // FCLEX, etc.
    /* DC */ eENTRY_CopyBytes2Mod,                  // FADD/0, etc.
    /* DD */ eENTRY_CopyBytes2Mod,                  // FFREE, etc.
    /* DE */ eENTRY_CopyBytes2Mod,                  // FADDP, etc.
    /* DF */ eENTRY_CopyBytes2Mod,                  // FBLD/4, etc.
    /* E0 */ eENTRY_CopyBytes2CantJump,             // LOOPNE cb
    /* E1 */ eENTRY_CopyBytes2CantJump,             // LOOPE cb
    /* E2 */ eENTRY_CopyBytes2CantJump,             // LOOP cb
    /* E3 */ eENTRY_CopyBytes2CantJump,             // JCXZ/JECXZ
    /* E4 */ eENTRY_CopyBytes2,                     // IN ib
    /* E5 */ eENTRY_CopyBytes2,                     // IN id
    /* E6 */ eENTRY_CopyBytes2,                     // OUT ib
    /* E7 */ eENTRY_CopyBytes2,                     // OUT ib
    /* E8 */ eENTRY_CopyBytes3Or5Target,            // CALL cd
    /* E9 */ eENTRY_CopyBytes3Or5Target,            // JMP cd
#if defined(_AMD64_)
    /* EA */ eENTRY_Invalid,                        // Invalid
#else
    /* EA */ eENTRY_CopyBytes5Or7Dynamic,           // JMP cp
#endif
    /* EB */ eENTRY_CopyBytes2Jump,                 // JMP cb
    /* EC */ eENTRY_CopyBytes1,                     // IN ib
    /* ED */ eENTRY_CopyBytes1,                     // IN id
    /* EE */ eENTRY_CopyBytes1,                     // OUT
    /* EF */ eENTRY_CopyBytes1,                     // OUT
    /* F0 */ eENTRY_CopyBytesPrefix,                // LOCK prefix
    /* F1 */ eENTRY_CopyBytes1Dynamic,              // INT1 / ICEBP somewhat documented by AMD, not by Intel
    /* F2 */ eENTRY_CopyF2,                         // REPNE prefix

    // This does presently suffice for AMD64 but it requires tracing
    // through a bunch of code to verify and seems not worth maintaining.
    // For x64: /* F3 */ eENTRY_CopyBytesPrefix
    /* F3 */ eENTRY_CopyF3,                         // REPE prefix

    /* F4 */ eENTRY_CopyBytes1,                     // HLT
    /* F5 */ eENTRY_CopyBytes1,                     // CMC
    /* F6 */ eENTRY_CopyF6,                         // TEST/0, DIV/6
    /* F7 */ eENTRY_CopyF7,                         // TEST/0, DIV/6
    /* F8 */ eENTRY_CopyBytes1,                     // CLC
    /* F9 */ eENTRY_CopyBytes1,                     // STC
    /* FA */ eENTRY_CopyBytes1,                     // CLI
    /* FB */ eENTRY_CopyBytes1,                     // STI
    /* FC */ eENTRY_CopyBytes1,                     // CLD
    /* FD */ eENTRY_CopyBytes1,                     // STD
    /* FE */ eENTRY_CopyBytes2Mod,                  // DEC/1,INC/0
    /* FF */ eENTRY_CopyFF,                         // CALL/2
};

static const BYTE g_rceCopyTable0F[] =
{
#if defined(_X86_)
    /* 00 */ eENTRY_Copy0F00,                       // sldt/0 str/1 lldt/2 ltr/3 err/4 verw/5 jmpe/6/dynamic invalid/7
#else
    /* 00 */ eENTRY_CopyBytes2Mod,                  // sldt/0 str/1 lldt/2 ltr/3 err/4 verw/5 jmpe/6/dynamic invalid/7
#endif
    /* 01 */ eENTRY_CopyBytes2Mod,                  // INVLPG/7, etc.
    /* 02 */ eENTRY_CopyBytes2Mod,                  // LAR/r
    /* 03 */ eENTRY_CopyBytes2Mod,                  // LSL/r
    /* 04 */ eENTRY_Invalid,                        // _04
    /* 05 */ eENTRY_CopyBytes1,                     // SYSCALL
    /* 06 */ eENTRY_CopyBytes1,                     // CLTS
    /* 07 */ eENTRY_CopyBytes1,                     // SYSRET
    /* 08 */ eENTRY_CopyBytes1,                     // INVD
    /* 09 */ eENTRY_CopyBytes1,                     // WBINVD
    /* 0A */ eENTRY_Invalid,                        // _0A
    /* 0B */ eENTRY_CopyBytes1,                     // UD2
    /* 0C */ eENTRY_Invalid,                        // _0C
    /* 0D */ eENTRY_CopyBytes2Mod,                  // PREFETCH
    /* 0E */ eENTRY_CopyBytes1,                     // FEMMS (3DNow -- not in Intel documentation)
    /* 0F */ eENTRY_CopyBytes2Mod1,                 // 3DNow Opcodes
    /* 10 */ eENTRY_CopyBytes2Mod,                  // MOVSS MOVUPD MOVSD
    /* 11 */ eENTRY_CopyBytes2Mod,                  // MOVSS MOVUPD MOVSD
    /* 12 */ eENTRY_CopyBytes2Mod,                  // MOVLPD
    /* 13 */ eENTRY_CopyBytes2Mod,                  // MOVLPD
    /* 14 */ eENTRY_CopyBytes2Mod,                  // UNPCKLPD
    /* 15 */ eENTRY_CopyBytes2Mod,                  // UNPCKHPD
    /* 16 */ eENTRY_CopyBytes2Mod,                  // MOVHPD
    /* 17 */ eENTRY_CopyBytes2Mod,                  // MOVHPD
    /* 18 */ eENTRY_CopyBytes2Mod,                  // PREFETCHINTA...
    /* 19 */ eENTRY_CopyBytes2Mod,                  // NOP/r multi byte nop, not documented by Intel, documented by AMD
    /* 1A */ eENTRY_CopyBytes2Mod,                  // NOP/r multi byte nop, not documented by Intel, documented by AMD
    /* 1B */ eENTRY_CopyBytes2Mod,                  // NOP/r multi byte nop, not documented by Intel, documented by AMD
    /* 1C */ eENTRY_CopyBytes2Mod,                  // NOP/r multi byte nop, not documented by Intel, documented by AMD
    /* 1D */ eENTRY_CopyBytes2Mod,                  // NOP/r multi byte nop, not documented by Intel, documented by AMD
    /* 1E */ eENTRY_CopyBytes2Mod,                  // NOP/r multi byte nop, not documented by Intel, documented by AMD
    /* 1F */ eENTRY_CopyBytes2Mod,                  // NOP/r multi byte nop
    /* 20 */ eENTRY_CopyBytes2Mod,                  // MOV/r
    /* 21 */ eENTRY_CopyBytes2Mod,                  // MOV/r
    /* 22 */ eENTRY_CopyBytes2Mod,                  // MOV/r
    /* 23 */ eENTRY_CopyBytes2Mod,                  // MOV/r
#if defined(_AMD64_)
    /* 24 */ eENTRY_Invalid,                        // _24
#else
    /* 24 */ eENTRY_CopyBytes2Mod,                  // MOV/r,TR TR is test register on 80386 and 80486, removed in Pentium
#endif
    /* 25 */ eENTRY_Invalid,                        // _25
#if defined(_AMD64_)
    /* 26 */ eENTRY_Invalid,                        // _26
#else
    /* 26 */ eENTRY_CopyBytes2Mod,                  // MOV TR/r TR is test register on 80386 and 80486, removed in Pentium
#endif
    /* 27 */ eENTRY_Invalid,                        // _27
    /* 28 */ eENTRY_CopyBytes2Mod,                  // MOVAPS MOVAPD
    /* 29 */ eENTRY_CopyBytes2Mod,                  // MOVAPS MOVAPD
    /* 2A */ eENTRY_CopyBytes2Mod,                  // CVPI2PS &
    /* 2B */ eENTRY_CopyBytes2Mod,                  // MOVNTPS MOVNTPD
    /* 2C */ eENTRY_CopyBytes2Mod,                  // CVTTPS2PI &
    /* 2D */ eENTRY_CopyBytes2Mod,                  // CVTPS2PI &
    /* 2E */ eENTRY_CopyBytes2Mod,                  // UCOMISS UCOMISD
    /* 2F */ eENTRY_CopyBytes2Mod,                  // COMISS COMISD
    /* 30 */ eENTRY_CopyBytes1,                     // WRMSR
    /* 31 */ eENTRY_CopyBytes1,                     // RDTSC
    /* 32 */ eENTRY_CopyBytes1,                     // RDMSR
    /* 33 */ eENTRY_CopyBytes1,                     // RDPMC
    /* 34 */ eENTRY_CopyBytes1,                     // SYSENTER
    /* 35 */ eENTRY_CopyBytes1,                     // SYSEXIT
    /* 36 */ eENTRY_Invalid,                        // _36
    /* 37 */ eENTRY_CopyBytes1,                     // GETSEC
    /* 38 */ eENTRY_CopyBytes3Mod,                  // SSE3 Opcodes
    /* 39 */ eENTRY_Invalid,                        // _39
    /* 3A */ eENTRY_CopyBytes3Mod1,                  // SSE3 Opcodes
    /* 3B */ eENTRY_Invalid,                        // _3B
    /* 3C */ eENTRY_Invalid,                        // _3C
    /* 3D */ eENTRY_Invalid,                        // _3D
    /* 3E */ eENTRY_Invalid,                        // _3E
    /* 3F */ eENTRY_Invalid,                        // _3F
    /* 40 */ eENTRY_CopyBytes2Mod,                  // CMOVO (0F 40)
    /* 41 */ eENTRY_CopyBytes2Mod,                  // CMOVNO (0F 41)
    /* 42 */ eENTRY_CopyBytes2Mod,                  // CMOVB & CMOVNE (0F 42)
    /* 43 */ eENTRY_CopyBytes2Mod,                  // CMOVAE & CMOVNB (0F 43)
    /* 44 */ eENTRY_CopyBytes2Mod,                  // CMOVE & CMOVZ (0F 44)
    /* 45 */ eENTRY_CopyBytes2Mod,                  // CMOVNE & CMOVNZ (0F 45)
    /* 46 */ eENTRY_CopyBytes2Mod,                  // CMOVBE & CMOVNA (0F 46)
    /* 47 */ eENTRY_CopyBytes2Mod,                  // CMOVA & CMOVNBE (0F 47)
    /* 48 */ eENTRY_CopyBytes2Mod,                  // CMOVS (0F 48)
    /* 49 */ eENTRY_CopyBytes2Mod,                  // CMOVNS (0F 49)
    /* 4A */ eENTRY_CopyBytes2Mod,                  // CMOVP & CMOVPE (0F 4A)
    /* 4B */ eENTRY_CopyBytes2Mod,                  // CMOVNP & CMOVPO (0F 4B)
    /* 4C */ eENTRY_CopyBytes2Mod,                  // CMOVL & CMOVNGE (0F 4C)
    /* 4D */ eENTRY_CopyBytes2Mod,                  // CMOVGE & CMOVNL (0F 4D)
    /* 4E */ eENTRY_CopyBytes2Mod,                  // CMOVLE & CMOVNG (0F 4E)
    /* 4F */ eENTRY_CopyBytes2Mod,                  // CMOVG & CMOVNLE (0F 4F)
    /* 50 */ eENTRY_CopyBytes2Mod,                  // MOVMSKPD MOVMSKPD
    /* 51 */ eENTRY_CopyBytes2Mod,                  // SQRTPS &
    /* 52 */ eENTRY_CopyBytes2Mod,                  // RSQRTTS RSQRTPS
    /* 53 */ eENTRY_CopyBytes2Mod,                  // RCPPS RCPSS
    /* 54 */ eENTRY_CopyBytes2Mod,                  // ANDPS ANDPD
    /* 55 */ eENTRY_CopyBytes2Mod,                  // ANDNPS ANDNPD
    /* 56 */ eENTRY_CopyBytes2Mod,                  // ORPS ORPD
    /* 57 */ eENTRY_CopyBytes2Mod,                  // XORPS XORPD
    /* 58 */ eENTRY_CopyBytes2Mod,                  // ADDPS &
    /* 59 */ eENTRY_CopyBytes2Mod,                  // MULPS &
    /* 5A */ eENTRY_CopyBytes2Mod,                  // CVTPS2PD &
    /* 5B */ eENTRY_CopyBytes2Mod,                  // CVTDQ2PS &
    /* 5C */ eENTRY_CopyBytes2Mod,                  // SUBPS &
    /* 5D */ eENTRY_CopyBytes2Mod,                  // MINPS &
    /* 5E */ eENTRY_CopyBytes2Mod,                  // DIVPS &
    /* 5F */ eENTRY_CopyBytes2Mod,                  // MASPS &
    /* 60 */ eENTRY_CopyBytes2Mod,                  // PUNPCKLBW/r
    /* 61 */ eENTRY_CopyBytes2Mod,                  // PUNPCKLWD/r
    /* 62 */ eENTRY_CopyBytes2Mod,                  // PUNPCKLWD/r
    /* 63 */ eENTRY_CopyBytes2Mod,                  // PACKSSWB/r
    /* 64 */ eENTRY_CopyBytes2Mod,                  // PCMPGTB/r
    /* 65 */ eENTRY_CopyBytes2Mod,                  // PCMPGTW/r
    /* 66 */ eENTRY_CopyBytes2Mod,                  // PCMPGTD/r
    /* 67 */ eENTRY_CopyBytes2Mod,                  // PACKUSWB/r
    /* 68 */ eENTRY_CopyBytes2Mod,                  // PUNPCKHBW/r
    /* 69 */ eENTRY_CopyBytes2Mod,                  // PUNPCKHWD/r
    /* 6A */ eENTRY_CopyBytes2Mod,                  // PUNPCKHDQ/r
    /* 6B */ eENTRY_CopyBytes2Mod,                  // PACKSSDW/r
    /* 6C */ eENTRY_CopyBytes2Mod,                  // PUNPCKLQDQ
    /* 6D */ eENTRY_CopyBytes2Mod,                  // PUNPCKHQDQ
    /* 6E */ eENTRY_CopyBytes2Mod,                  // MOVD/r
    /* 6F */ eENTRY_CopyBytes2Mod,                  // MOV/r
    /* 70 */ eENTRY_CopyBytes2Mod1,                 // PSHUFW/r ib
    /* 71 */ eENTRY_CopyBytes2Mod1,                 // PSLLW/6 ib,PSRAW/4 ib,PSRLW/2 ib
    /* 72 */ eENTRY_CopyBytes2Mod1,                 // PSLLD/6 ib,PSRAD/4 ib,PSRLD/2 ib
    /* 73 */ eENTRY_CopyBytes2Mod1,                 // PSLLQ/6 ib,PSRLQ/2 ib
    /* 74 */ eENTRY_CopyBytes2Mod,                  // PCMPEQB/r
    /* 75 */ eENTRY_CopyBytes2Mod,                  // PCMPEQW/r
    /* 76 */ eENTRY_CopyBytes2Mod,                  // PCMPEQD/r
    /* 77 */ eENTRY_CopyBytes1,                     // EMMS
    // extrq/insertq require mode=3 and are followed by two immediate bytes
    /* 78 */ eENTRY_Copy0F78,                       // VMREAD/r, 66/EXTRQ/r/ib/ib, F2/INSERTQ/r/ib/ib
    // extrq/insertq require mod=3, therefore eENTRY_CopyBytes2, but it ends up the same
    /* 79 */ eENTRY_CopyBytes2Mod,                  // VMWRITE/r, 66/EXTRQ/r, F2/INSERTQ/r
    /* 7A */ eENTRY_Invalid,                        // _7A
    /* 7B */ eENTRY_Invalid,                        // _7B
    /* 7C */ eENTRY_CopyBytes2Mod,                  // HADDPS
    /* 7D */ eENTRY_CopyBytes2Mod,                  // HSUBPS
    /* 7E */ eENTRY_CopyBytes2Mod,                  // MOVD/r
    /* 7F */ eENTRY_CopyBytes2Mod,                  // MOV/r
    /* 80 */ eENTRY_CopyBytes3Or5Target,            // JO
    /* 81 */ eENTRY_CopyBytes3Or5Target,            // JNO
    /* 82 */ eENTRY_CopyBytes3Or5Target,            // JB,JC,JNAE
    /* 83 */ eENTRY_CopyBytes3Or5Target,            // JAE,JNB,JNC
    /* 84 */ eENTRY_CopyBytes3Or5Target,            // JE,JZ,JZ
    /* 85 */ eENTRY_CopyBytes3Or5Target,            // JNE,JNZ
    /* 86 */ eENTRY_CopyBytes3Or5Target,            // JBE,JNA
    /* 87 */ eENTRY_CopyBytes3Or5Target,            // JA,JNBE
    /* 88 */ eENTRY_CopyBytes3Or5Target,            // JS
    /* 89 */ eENTRY_CopyBytes3Or5Target,            // JNS
    /* 8A */ eENTRY_CopyBytes3Or5Target,            // JP,JPE
    /* 8B */ eENTRY_CopyBytes3Or5Target,            // JNP,JPO
    /* 8C */ eENTRY_CopyBytes3Or5Target,            // JL,NGE
    /* 8D */ eENTRY_CopyBytes3Or5Target,            // JGE,JNL
    /* 8E */ eENTRY_CopyBytes3Or5Target,            // JLE,JNG
    /* 8F */ eENTRY_CopyBytes3Or5Target,            // JG,JNLE
    /* 90 */ eENTRY_CopyBytes2Mod,                  // CMOVO (0F 40)
    /* 91 */ eENTRY_CopyBytes2Mod,                  // CMOVNO (0F 41)
    /* 92 */ eENTRY_CopyBytes2Mod,                  // CMOVB & CMOVC & CMOVNAE (0F 42)
    /* 93 */ eENTRY_CopyBytes2Mod,                  // CMOVAE & CMOVNB & CMOVNC (0F 43)
    /* 94 */ eENTRY_CopyBytes2Mod,                  // CMOVE & CMOVZ (0F 44)
    /* 95 */ eENTRY_CopyBytes2Mod,                  // CMOVNE & CMOVNZ (0F 45)
    /* 96 */ eENTRY_CopyBytes2Mod,                  // CMOVBE & CMOVNA (0F 46)
    /* 97 */ eENTRY_CopyBytes2Mod,                  // CMOVA & CMOVNBE (0F 47)
    /* 98 */ eENTRY_CopyBytes2Mod,                  // CMOVS (0F 48)
    /* 99 */ eENTRY_CopyBytes2Mod,                  // CMOVNS (0F 49)
    /* 9A */ eENTRY_CopyBytes2Mod,                  // CMOVP & CMOVPE (0F 4A)
    /* 9B */ eENTRY_CopyBytes2Mod,                  // CMOVNP & CMOVPO (0F 4B)
    /* 9C */ eENTRY_CopyBytes2Mod,                  // CMOVL & CMOVNGE (0F 4C)
    /* 9D */ eENTRY_CopyBytes2Mod,                  // CMOVGE & CMOVNL (0F 4D)
    /* 9E */ eENTRY_CopyBytes2Mod,                  // CMOVLE & CMOVNG (0F 4E)
    /* 9F */ eENTRY_CopyBytes2Mod,                  // CMOVG & CMOVNLE (0F 4F)
    /* A0 */ eENTRY_CopyBytes1,                     // PUSH
    /* A1 */ eENTRY_CopyBytes1,                     // POP
    /* A2 */ eENTRY_CopyBytes1,                     // CPUID
    /* A3 */ eENTRY_CopyBytes2Mod,                  // BT  (0F A3)
    /* A4 */ eENTRY_CopyBytes2Mod1,                 // SHLD
    /* A5 */ eENTRY_CopyBytes2Mod,                  // SHLD
    /* A6 */ eENTRY_CopyBytes2Mod,                  // XBTS
    /* A7 */ eENTRY_CopyBytes2Mod,                  // IBTS
    /* A8 */ eENTRY_CopyBytes1,                     // PUSH
    /* A9 */ eENTRY_CopyBytes1,                     // POP
    /* AA */ eENTRY_CopyBytes1,                     // RSM
    /* AB */ eENTRY_CopyBytes2Mod,                  // BTS (0F AB)
    /* AC */ eENTRY_CopyBytes2Mod1,                 // SHRD
    /* AD */ eENTRY_CopyBytes2Mod,                  // SHRD

    // 0F AE mod76=mem mod543=0 fxsave
    // 0F AE mod76=mem mod543=1 fxrstor
    // 0F AE mod76=mem mod543=2 ldmxcsr
    // 0F AE mod76=mem mod543=3 stmxcsr
    // 0F AE mod76=mem mod543=4 xsave
    // 0F AE mod76=mem mod543=5 xrstor
    // 0F AE mod76=mem mod543=6 saveopt
    // 0F AE mod76=mem mod543=7 clflush
    // 0F AE mod76=11b mod543=5 lfence
    // 0F AE mod76=11b mod543=6 mfence
    // 0F AE mod76=11b mod543=7 sfence
    // F3 0F AE mod76=11b mod543=0 rdfsbase
    // F3 0F AE mod76=11b mod543=1 rdgsbase
    // F3 0F AE mod76=11b mod543=2 wrfsbase
    // F3 0F AE mod76=11b mod543=3 wrgsbase
    /* AE */ eENTRY_CopyBytes2Mod,                  // fxsave fxrstor ldmxcsr stmxcsr xsave xrstor saveopt clflush lfence mfence sfence rdfsbase rdgsbase wrfsbase wrgsbase
    /* AF */ eENTRY_CopyBytes2Mod,                  // IMUL (0F AF)
    /* B0 */ eENTRY_CopyBytes2Mod,                  // CMPXCHG (0F B0)
    /* B1 */ eENTRY_CopyBytes2Mod,                  // CMPXCHG (0F B1)
    /* B2 */ eENTRY_CopyBytes2Mod,                  // LSS/r
    /* B3 */ eENTRY_CopyBytes2Mod,                  // BTR (0F B3)
    /* B4 */ eENTRY_CopyBytes2Mod,                  // LFS/r
    /* B5 */ eENTRY_CopyBytes2Mod,                  // LGS/r
    /* B6 */ eENTRY_CopyBytes2Mod,                  // MOVZX/r
    /* B7 */ eENTRY_CopyBytes2Mod,                  // MOVZX/r
#if defined(_X86_)
    /* B8 */ eENTRY_Copy0FB8,                       // jmpe f3/popcnt
#else
    /* B8 */ eENTRY_CopyBytes2Mod,                  // f3/popcnt
#endif
    /* B9 */ eENTRY_Invalid,                        // _B9
    /* BA */ eENTRY_CopyBytes2Mod1,                 // BT & BTC & BTR & BTS (0F BA)
    /* BB */ eENTRY_CopyBytes2Mod,                  // BTC (0F BB)
    /* BC */ eENTRY_CopyBytes2Mod,                  // BSF (0F BC)
    /* BD */ eENTRY_CopyBytes2Mod,                  // BSR (0F BD)
    /* BE */ eENTRY_CopyBytes2Mod,                  // MOVSX/r
    /* BF */ eENTRY_CopyBytes2Mod,                  // MOVSX/r
    /* C0 */ eENTRY_CopyBytes2Mod,                  // XADD/r
    /* C1 */ eENTRY_CopyBytes2Mod,                  // XADD/r
    /* C2 */ eENTRY_CopyBytes2Mod1,                 // CMPPS &
    /* C3 */ eENTRY_CopyBytes2Mod,                  // MOVNTI
    /* C4 */ eENTRY_CopyBytes2Mod1,                 // PINSRW /r ib
    /* C5 */ eENTRY_CopyBytes2Mod1,                 // PEXTRW /r ib
    /* C6 */ eENTRY_CopyBytes2Mod1,                 // SHUFPS & SHUFPD
    /* C7 */ eENTRY_CopyBytes2Mod,                  // CMPXCHG8B (0F C7)
    /* C8 */ eENTRY_CopyBytes1,                     // BSWAP 0F C8 + rd
    /* C9 */ eENTRY_CopyBytes1,                     // BSWAP 0F C8 + rd
    /* CA */ eENTRY_CopyBytes1,                     // BSWAP 0F C8 + rd
    /* CB */ eENTRY_CopyBytes1,                     // CVTPD2PI BSWAP 0F C8 + rd
    /* CC */ eENTRY_CopyBytes1,                     // BSWAP 0F C8 + rd
    /* CD */ eENTRY_CopyBytes1,                     // BSWAP 0F C8 + rd
    /* CE */ eENTRY_CopyBytes1,                     // BSWAP 0F C8 + rd
    /* CF */ eENTRY_CopyBytes1,                     // BSWAP 0F C8 + rd
    /* D0 */ eENTRY_CopyBytes2Mod,                  // ADDSUBPS (untestd)
    /* D1 */ eENTRY_CopyBytes2Mod,                  // PSRLW/r
    /* D2 */ eENTRY_CopyBytes2Mod,                  // PSRLD/r
    /* D3 */ eENTRY_CopyBytes2Mod,                  // PSRLQ/r
    /* D4 */ eENTRY_CopyBytes2Mod,                  // PADDQ
    /* D5 */ eENTRY_CopyBytes2Mod,                  // PMULLW/r
    /* D6 */ eENTRY_CopyBytes2Mod,                  // MOVDQ2Q / MOVQ2DQ
    /* D7 */ eENTRY_CopyBytes2Mod,                  // PMOVMSKB/r
    /* D8 */ eENTRY_CopyBytes2Mod,                  // PSUBUSB/r
    /* D9 */ eENTRY_CopyBytes2Mod,                  // PSUBUSW/r
    /* DA */ eENTRY_CopyBytes2Mod,                  // PMINUB/r
    /* DB */ eENTRY_CopyBytes2Mod,                  // PAND/r
    /* DC */ eENTRY_CopyBytes2Mod,                  // PADDUSB/r
    /* DD */ eENTRY_CopyBytes2Mod,                  // PADDUSW/r
    /* DE */ eENTRY_CopyBytes2Mod,                  // PMAXUB/r
    /* DF */ eENTRY_CopyBytes2Mod,                  // PANDN/r
    /* E0 */ eENTRY_CopyBytes2Mod,                  // PAVGB
    /* E1 */ eENTRY_CopyBytes2Mod,                  // PSRAW/r
    /* E2 */ eENTRY_CopyBytes2Mod,                  // PSRAD/r
    /* E3 */ eENTRY_CopyBytes2Mod,                  // PAVGW
    /* E4 */ eENTRY_CopyBytes2Mod,                  // PMULHUW/r
    /* E5 */ eENTRY_CopyBytes2Mod,                  // PMULHW/r
    /* E6 */ eENTRY_CopyBytes2Mod,                  // CTDQ2PD &
    /* E7 */ eENTRY_CopyBytes2Mod,                  // MOVNTQ
    /* E8 */ eENTRY_CopyBytes2Mod,                  // PSUBB/r
    /* E9 */ eENTRY_CopyBytes2Mod,                  // PSUBW/r
    /* EA */ eENTRY_CopyBytes2Mod,                  // PMINSW/r
    /* EB */ eENTRY_CopyBytes2Mod,                  // POR/r
    /* EC */ eENTRY_CopyBytes2Mod,                  // PADDSB/r
    /* ED */ eENTRY_CopyBytes2Mod,                  // PADDSW/r
    /* EE */ eENTRY_CopyBytes2Mod,                  // PMAXSW /r
    /* EF */ eENTRY_CopyBytes2Mod,                  // PXOR/r
    /* F0 */ eENTRY_CopyBytes2Mod,                  // LDDQU
    /* F1 */ eENTRY_CopyBytes2Mod,                  // PSLLW/r
    /* F2 */ eENTRY_CopyBytes2Mod,                  // PSLLD/r
    /* F3 */ eENTRY_CopyBytes2Mod,                  // PSLLQ/r
    /* F4 */ eENTRY_CopyBytes2Mod,                  // PMULUDQ/r
    /* F5 */ eENTRY_CopyBytes2Mod,                  // PMADDWD/r
    /* F6 */ eENTRY_CopyBytes2Mod,                  // PSADBW/r
    /* F7 */ eENTRY_CopyBytes2Mod,                  // MASKMOVQ
    /* F8 */ eENTRY_CopyBytes2Mod,                  // PSUBB/r
    /* F9 */ eENTRY_CopyBytes2Mod,                  // PSUBW/r
    /* FA */ eENTRY_CopyBytes2Mod,                  // PSUBD/r
    /* FB */ eENTRY_CopyBytes2Mod,                  // FSUBQ/r
    /* FC */ eENTRY_CopyBytes2Mod,                  // PADDB/r
    /* FD */ eENTRY_CopyBytes2Mod,                  // PADDW/r
    /* FE */ eENTRY_CopyBytes2Mod,                  // PADDD/r
    /* FF */ eENTRY_Invalid,                        // _FF
};

_STATIC_ASSERT(_countof(g_rbModRm) == 256 &&
               _countof(g_rceCopyMap) == eENTRY_Invalid + 1 &&
               _countof(g_rceCopyTable) == 256 &&
               _countof(g_rceCopyTable0F) == 256);

/////////////////////////////////////////////////////////// Disassembler Code.
//

static
PBYTE
AdjustTarget(
    _In_ PDETOUR_DISASM pDisasm,
    _In_ PBYTE pbDst,
    _In_ PBYTE pbSrc,
    UINT cbOp,
    UINT cbTargetOffset,
    UINT cbTargetSize)
{
    PBYTE pbTarget = NULL;
    LONG_PTR nOldOffset;
    LONG_PTR nNewOffset;
    PVOID pvTargetAddr = &pbDst[cbTargetOffset];

    switch (cbTargetSize)
    {
        case 1:
            nOldOffset = *(signed char*)pvTargetAddr;
            break;
        case 2:
            nOldOffset = *(UNALIGNED SHORT*)pvTargetAddr;
            break;
        case 4:
            nOldOffset = *(UNALIGNED LONG*)pvTargetAddr;
            break;
#if defined(_AMD64_)
        case 8:
            nOldOffset = *(UNALIGNED LONGLONG*)pvTargetAddr;
            break;
#endif
        default:
            ASSERT(!"cbTargetSize is invalid.");
            nOldOffset = 0;
            break;
    }

    pbTarget = pbSrc + cbOp + nOldOffset;
    nNewOffset = nOldOffset - (LONG_PTR)(pbDst - pbSrc);

    switch (cbTargetSize)
    {
        case 1:
            *(CHAR*)pvTargetAddr = (CHAR)nNewOffset;
            if (nNewOffset < SCHAR_MIN || nNewOffset > SCHAR_MAX)
            {
                *pDisasm->plExtra = sizeof(ULONG) - 1;
            }
            break;
        case 2:
            *(UNALIGNED SHORT*)pvTargetAddr = (SHORT)nNewOffset;
            if (nNewOffset < SHRT_MIN || nNewOffset > SHRT_MAX)
            {
                *pDisasm->plExtra = sizeof(ULONG) - 2;
            }
            break;
        case 4:
            *(UNALIGNED LONG*)pvTargetAddr = (LONG)nNewOffset;
            if (nNewOffset < LONG_MIN || nNewOffset > LONG_MAX)
            {
                *pDisasm->plExtra = sizeof(ULONG) - 4;
            }
            break;
#if defined(_AMD64_)
        case 8:
            *(UNALIGNED LONGLONG*)pvTargetAddr = nNewOffset;
            break;
#endif
    }
#if defined(_AMD64_)
    // When we are only computing size, source and dest can be
    // far apart, distance not encodable in 32bits. Ok.
    // At least still check the lower 32bits.

    if (pbDst >= pDisasm->rbScratchDst && pbDst < (sizeof(pDisasm->rbScratchDst) + pDisasm->rbScratchDst))
    {
        ASSERT((((size_t)pbDst + cbOp + nNewOffset) & 0xFFFFFFFF) == (((size_t)pbTarget) & 0xFFFFFFFF));
    } else
#endif
    {
        ASSERT(pbDst + cbOp + nNewOffset == pbTarget);
    }
    return pbTarget;
}

static
PBYTE
Invalid(
    _In_ PDETOUR_DISASM pDisasm,
    _In_opt_ REFCOPYENTRY pEntry,
    _In_ PBYTE pbDst,
    _In_ PBYTE pbSrc)
{
    UNREFERENCED_PARAMETER(pDisasm);
    UNREFERENCED_PARAMETER(pEntry);
    UNREFERENCED_PARAMETER(pbDst);
    UNREFERENCED_PARAMETER(pbSrc);

    return NULL;
}

static
PBYTE
CopyInstruction(
    _In_ PDETOUR_DISASM pDisasm,
    _In_opt_ PBYTE pbDst,
    _In_ PBYTE pbSrc)
{
    // Configure scratch areas if real areas are not available.
    if (NULL == pbDst)
    {
        pbDst = pDisasm->rbScratchDst;
    }

    // Figure out how big the instruction is, do the appropriate copy,
    // and figure out what the target of the instruction is if any.
    //
    const COPYENTRY* ce = &g_rceCopyMap[g_rceCopyTable[pbSrc[0]]];
    return ce->pfCopy(pDisasm, ce, pbDst, pbSrc);
}

static
PBYTE
CopyBytes(
    _In_ PDETOUR_DISASM pDisasm,
    _In_opt_ REFCOPYENTRY pEntry,
    _In_ PBYTE pbDst,
    _In_ PBYTE pbSrc)
{
    UINT nBytesFixed;

    if (pDisasm->bVex || pDisasm->bEvex)
    {
        ASSERT(pEntry->nFlagBits == 0);
        ASSERT(pEntry->nFixedSize == pEntry->nFixedSize16);
    }

    UINT const nModOffset = pEntry->nModOffset;
    UINT const nFlagBits = pEntry->nFlagBits;
    UINT const nFixedSize = pEntry->nFixedSize;
    UINT const nFixedSize16 = pEntry->nFixedSize16;

    if (nFlagBits & ADDRESS)
    {
        nBytesFixed = pDisasm->bAddressOverride ? nFixedSize16 : nFixedSize;
    }
#if defined(_AMD64_)
    // REX.W trumps 66
    else if (pDisasm->bRaxOverride)
    {
        nBytesFixed = nFixedSize + ((nFlagBits & RAX) ? 4 : 0);
    }
#endif
    else
    {
        nBytesFixed = pDisasm->bOperandOverride ? nFixedSize16 : nFixedSize;
    }

    UINT nBytes = nBytesFixed;
    UINT nRelOffset = pEntry->nRelOffset;
    UINT cbTarget = nBytes - nRelOffset;
    if (nModOffset > 0)
    {
        ASSERT(nRelOffset == 0);
        BYTE const bModRm = pbSrc[nModOffset];
        BYTE const bFlags = g_rbModRm[bModRm];

        nBytes += bFlags & NOTSIB;

        if (bFlags & SIB)
        {
            BYTE const bSib = pbSrc[nModOffset + 1];

            if ((bSib & 0x07) == 0x05)
            {
                if ((bModRm & 0xc0) == 0x00)
                {
                    nBytes += 4;
                } else if ((bModRm & 0xc0) == 0x40)
                {
                    nBytes += 1;
                } else if ((bModRm & 0xc0) == 0x80)
                {
                    nBytes += 4;
                }
            }
            cbTarget = nBytes - nRelOffset;
        }
#if defined(_AMD64_)
        else if (bFlags & RIP)
        {
            nRelOffset = nModOffset + 1;
            cbTarget = 4;
        }
#endif
    }
    CopyMemory(pbDst, pbSrc, nBytes);

    if (nRelOffset)
    {
        *pDisasm->ppbTarget = AdjustTarget(pDisasm, pbDst, pbSrc, nBytes, nRelOffset, cbTarget);
#if defined(_AMD64_)
        if (pEntry->nRelOffset == 0)
        {
            // This is a data target, not a code target, so we shouldn't return it.
            *pDisasm->ppbTarget = NULL;
        }
#endif
    }
    if (nFlagBits & NOENLARGE)
    {
        *pDisasm->plExtra = -*pDisasm->plExtra;
    }
    if (nFlagBits & DYNAMIC)
    {
        *pDisasm->ppbTarget = (PBYTE)DETOUR_INSTRUCTION_TARGET_DYNAMIC;
    }
    return pbSrc + nBytes;
}

static
PBYTE
CopyBytesPrefix(
    _In_ PDETOUR_DISASM pDisasm,
    _In_opt_ REFCOPYENTRY pEntry,
    _In_ PBYTE pbDst, _In_ PBYTE pbSrc)
{
    UNREFERENCED_PARAMETER(pEntry);

    pbDst[0] = pbSrc[0];

    REFCOPYENTRY ce = &g_rceCopyMap[g_rceCopyTable[pbSrc[1]]];
    return ce->pfCopy(pDisasm, ce, pbDst + 1, pbSrc + 1);
}

static
PBYTE
CopyBytesSegment(
    _In_ PDETOUR_DISASM pDisasm,
    _In_opt_ REFCOPYENTRY pEntry,
    _In_ PBYTE pbDst,
    _In_ PBYTE pbSrc)
{
    UNREFERENCED_PARAMETER(pEntry);

    pDisasm->nSegmentOverride = pbSrc[0];
    return CopyBytesPrefix(pDisasm, NULL, pbDst, pbSrc);
}

static
PBYTE
CopyBytesRax(
    _In_ PDETOUR_DISASM pDisasm,
    _In_opt_ REFCOPYENTRY pEntry,
    _In_ PBYTE pbDst,
    _In_ PBYTE pbSrc)
{
    // AMD64 only

    UNREFERENCED_PARAMETER(pEntry);

    if (pbSrc[0] & 0x8)
    {
        pDisasm->bRaxOverride = TRUE;
    }
    return CopyBytesPrefix(pDisasm, NULL, pbDst, pbSrc);
}

static
PBYTE
CopyBytesJump(
    _In_ PDETOUR_DISASM pDisasm,
    _In_opt_ REFCOPYENTRY pEntry,
    _In_ PBYTE pbDst,
    _In_ PBYTE pbSrc)
{
    UNREFERENCED_PARAMETER(pEntry);

    PVOID pvSrcAddr = &pbSrc[1];
    PVOID pvDstAddr = NULL;
    LONG_PTR nOldOffset = (LONG_PTR)(*(signed char*)pvSrcAddr);
    LONG_PTR nNewOffset = 0;

    *pDisasm->ppbTarget = pbSrc + 2 + nOldOffset;

    if (pbSrc[0] == 0xeb)
    {
        pbDst[0] = 0xe9;
        pvDstAddr = &pbDst[1];
        nNewOffset = nOldOffset - ((pbDst - pbSrc) + 3);
        *(UNALIGNED LONG*)pvDstAddr = (LONG)nNewOffset;

        *pDisasm->plExtra = 3;
        return pbSrc + 2;
    }

    ASSERT(pbSrc[0] >= 0x70 && pbSrc[0] <= 0x7f);

    pbDst[0] = 0x0f;
    pbDst[1] = 0x80 | (pbSrc[0] & 0xf);
    pvDstAddr = &pbDst[2];
    nNewOffset = nOldOffset - ((pbDst - pbSrc) + 4);
    *(UNALIGNED LONG*)pvDstAddr = (LONG)nNewOffset;

    *pDisasm->plExtra = 4;
    return pbSrc + 2;
}

////////////////////////////////////////////////////// Individual Bytes Codes.
//
static
PBYTE
Copy0F(
    _In_ PDETOUR_DISASM pDisasm,
    _In_opt_ REFCOPYENTRY pEntry,
    _In_ PBYTE pbDst,
    _In_ PBYTE pbSrc)
{
    UNREFERENCED_PARAMETER(pEntry);

    pbDst[0] = pbSrc[0];

    REFCOPYENTRY ce = &g_rceCopyMap[g_rceCopyTable0F[pbSrc[1]]];
    return ce->pfCopy(pDisasm, ce, pbDst + 1, pbSrc + 1);
}

static
PBYTE
Copy0F78(
    _In_ PDETOUR_DISASM pDisasm,
    _In_opt_ REFCOPYENTRY pEntry,
    _In_ PBYTE pbDst,
    _In_ PBYTE pbSrc)
{
    // vmread, 66/extrq, F2/insertq

    UNREFERENCED_PARAMETER(pEntry);

    const BYTE vmread = /* 78 */ eENTRY_CopyBytes2Mod;
    const BYTE extrq_insertq = /* 78 */ eENTRY_CopyBytes4;

    ASSERT(!(pDisasm->bF2 && pDisasm->bOperandOverride));

    // For insertq and presumably despite documentation extrq, mode must be 11, not checked.
    // insertq/extrq/78 are followed by two immediate bytes, and given mode == 11, mod/rm byte is always one byte,
    // and the 0x78 makes 4 bytes (not counting the 66/F2/F which are accounted for elsewhere)

    REFCOPYENTRY ce = &g_rceCopyMap[((pDisasm->bF2 || pDisasm->bOperandOverride) ? extrq_insertq : vmread)];

    return ce->pfCopy(pDisasm, ce, pbDst, pbSrc);
}

static
PBYTE
Copy0F00(
    _In_ PDETOUR_DISASM pDisasm,
    _In_opt_ REFCOPYENTRY pEntry,
    _In_ PBYTE pbDst,
    _In_ PBYTE pbSrc)
{
    // jmpe is 32bit x86 only
    // Notice that the sizes are the same either way, but jmpe is marked as "dynamic".

    UNREFERENCED_PARAMETER(pEntry);

    const BYTE other = /* B8 */ eENTRY_CopyBytes2Mod; // sldt/0 str/1 lldt/2 ltr/3 err/4 verw/5 jmpe/6 invalid/7
    const BYTE jmpe = /* B8 */ eENTRY_CopyBytes2ModDynamic; // jmpe/6 x86-on-IA64 syscalls

    REFCOPYENTRY ce = &g_rceCopyMap[(((6 << 3) == ((7 << 3) & pbSrc[1])) ? jmpe : other)];
    return ce->pfCopy(pDisasm, ce, pbDst, pbSrc);
}

static
PBYTE
Copy0FB8(
    _In_ PDETOUR_DISASM pDisasm,
    _In_opt_ REFCOPYENTRY pEntry,
    _In_ PBYTE pbDst,
    _In_ PBYTE pbSrc)
{
    // jmpe is 32bit x86 only

    UNREFERENCED_PARAMETER(pEntry);

    const BYTE popcnt = /* B8 */ eENTRY_CopyBytes2Mod;
    const BYTE jmpe = /* B8 */ eENTRY_CopyBytes3Or5Dynamic; // jmpe x86-on-IA64 syscalls
    REFCOPYENTRY ce = &g_rceCopyMap[pDisasm->bF3 ? popcnt : jmpe];
    return ce->pfCopy(pDisasm, ce, pbDst, pbSrc);
}

static
PBYTE
Copy66(
    _In_ PDETOUR_DISASM pDisasm,
    _In_opt_ REFCOPYENTRY pEntry,
    _In_ PBYTE pbDst,
    _In_ PBYTE pbSrc)
{
    // Operand-size override prefix

    UNREFERENCED_PARAMETER(pEntry);

    pDisasm->bOperandOverride = TRUE;
    return CopyBytesPrefix(pDisasm, NULL, pbDst, pbSrc);
}

static
PBYTE
Copy67(
    _In_ PDETOUR_DISASM pDisasm,
    _In_opt_ REFCOPYENTRY pEntry,
    _In_ PBYTE pbDst,
    _In_ PBYTE pbSrc)
{
    // Address size override prefix

    UNREFERENCED_PARAMETER(pEntry);

    pDisasm->bAddressOverride = TRUE;
    return CopyBytesPrefix(pDisasm, NULL, pbDst, pbSrc);
}

static
PBYTE
CopyF2(
    _In_ PDETOUR_DISASM pDisasm,
    _In_opt_ REFCOPYENTRY pEntry,
    _In_ PBYTE pbDst,
    _In_ PBYTE pbSrc)
{
    UNREFERENCED_PARAMETER(pEntry);

    pDisasm->bF2 = TRUE;
    return CopyBytesPrefix(pDisasm, NULL, pbDst, pbSrc);
}

static
PBYTE
CopyF3(
    _In_ PDETOUR_DISASM pDisasm,
    _In_opt_ REFCOPYENTRY pEntry,
    _In_ PBYTE pbDst,
    _In_ PBYTE pbSrc)
{
    // x86 only

    UNREFERENCED_PARAMETER(pEntry);

    pDisasm->bF3 = TRUE;
    return CopyBytesPrefix(pDisasm, NULL, pbDst, pbSrc);
}

static
PBYTE
CopyF6(
    _In_ PDETOUR_DISASM pDisasm,
    _In_opt_ REFCOPYENTRY pEntry,
    _In_ PBYTE pbDst,
    _In_ PBYTE pbSrc)
{
    UNREFERENCED_PARAMETER(pEntry);

    // TEST BYTE /0
    if (0x00 == (0x38 & pbSrc[1]))
    {
        // reg(bits 543) of ModR/M == 0
        REFCOPYENTRY ce = /* f6 */ &g_rceCopyMap[eENTRY_CopyBytes2Mod1];
        return ce->pfCopy(pDisasm, ce, pbDst, pbSrc);
    } else
    {
        // DIV /6
        // IDIV /7
        // IMUL /5
        // MUL /4
        // NEG /3
        // NOT /2
        REFCOPYENTRY ce = /* f6 */ &g_rceCopyMap[eENTRY_CopyBytes2Mod];
        return ce->pfCopy(pDisasm, ce, pbDst, pbSrc);
    }
}

static
PBYTE
CopyF7(
    _In_ PDETOUR_DISASM pDisasm,
    _In_opt_ REFCOPYENTRY pEntry,
    _In_ PBYTE pbDst,
    _In_ PBYTE pbSrc)
{
    UNREFERENCED_PARAMETER(pEntry);

    // TEST WORD /0
    if (0x00 == (0x38 & pbSrc[1]))
    {
        // reg(bits 543) of ModR/M == 0
        REFCOPYENTRY ce = /* f7 */ &g_rceCopyMap[eENTRY_CopyBytes2ModOperand];
        return ce->pfCopy(pDisasm, ce, pbDst, pbSrc);
    } else
    {
        // DIV /6
        // IDIV /7
        // IMUL /5
        // MUL /4
        // NEG /3
        // NOT /2
        REFCOPYENTRY ce = /* f7 */ &g_rceCopyMap[eENTRY_CopyBytes2Mod];
        return ce->pfCopy(pDisasm, ce, pbDst, pbSrc);
    }
}

static
PBYTE
CopyFF(
    _In_ PDETOUR_DISASM pDisasm,
    _In_opt_ REFCOPYENTRY pEntry,
    _In_ PBYTE pbDst,
    _In_ PBYTE pbSrc)
{
    // INC /0
    // DEC /1
    // CALL /2
    // CALL /3
    // JMP /4
    // JMP /5
    // PUSH /6
    // invalid/7
    UNREFERENCED_PARAMETER(pEntry);

    REFCOPYENTRY ce = /* ff */ &g_rceCopyMap[eENTRY_CopyBytes2Mod];
    PBYTE pbOut = ce->pfCopy(pDisasm, ce, pbDst, pbSrc);

    BYTE const b1 = pbSrc[1];

    if (0x15 == b1 || 0x25 == b1)
    {
        // CALL [], JMP []
#if defined(_AMD64_)
        // All segments but FS and GS are equivalent.
        if (pDisasm->nSegmentOverride != 0x64 && pDisasm->nSegmentOverride != 0x65)
#else
        if (pDisasm->nSegmentOverride == 0 || pDisasm->nSegmentOverride == 0x2E)
#endif
        {
#if defined(_AMD64_)
            INT32 offset = *(UNALIGNED INT32*) & pbSrc[2];
            PBYTE* ppbTarget = (PBYTE*)(pbSrc + 6 + offset);
#else
            PBYTE* ppbTarget = (PBYTE*)(SIZE_T) * (UNALIGNED ULONG*) & pbSrc[2];
#endif
            // This can access violate on random bytes. Use DetourSetCodeModule.
            *pDisasm->ppbTarget = *ppbTarget;
        } else
        {
            *pDisasm->ppbTarget = (PBYTE)DETOUR_INSTRUCTION_TARGET_DYNAMIC;
        }
    } else if (0x10 == (0x30 & b1) || // CALL /2 or /3  --> reg(bits 543) of ModR/M == 010 or 011
               0x20 == (0x30 & b1))
    {
        // JMP /4 or /5 --> reg(bits 543) of ModR/M == 100 or 101
        *pDisasm->ppbTarget = (PBYTE)DETOUR_INSTRUCTION_TARGET_DYNAMIC;
    }
    return pbOut;
}

static
PBYTE
CopyVexEvexCommon(
    _In_ PDETOUR_DISASM pDisasm,
    BYTE m,
    _In_ PBYTE pbDst,
    _In_ PBYTE pbSrc,
    BYTE p,
    _In_opt_ BYTE fp16)
// m is first instead of last in the hopes of pbDst/pbSrc being
// passed along efficiently in the registers they were already in.
{
    REFCOPYENTRY ce;

    switch (p & 3)
    {
        case 0:
            break;
        case 1:
            pDisasm->bOperandOverride = TRUE;
            break;
        case 2:
            pDisasm->bF3 = TRUE;
            break;
        case 3:
            pDisasm->bF2 = TRUE;
            break;
    }

    // see https://software.intel.com/content/www/us/en/develop/download/intel-avx512-fp16-architecture-specification.html
    switch (m | fp16)
    {
        case 1:
            ce = &g_rceCopyMap[g_rceCopyTable0F[pbSrc[0]]];
            return ce->pfCopy(pDisasm, ce, pbDst, pbSrc);
        case 5: // fallthrough
        case 6: // fallthrough
        case 2:
            return CopyBytes(pDisasm, &g_rceCopyMap[eENTRY_CopyBytes2Mod], pbDst, pbSrc); /* 38 ceF38 */
        case 3:
            return CopyBytes(pDisasm, &g_rceCopyMap[eENTRY_CopyBytes2Mod1], pbDst, pbSrc); /* 3A ceF3A */
        default:
            return Invalid(pDisasm, &g_rceCopyMap[eENTRY_Invalid], pbDst, pbSrc); /* C4 ceInvalid */
    }
}

static
PBYTE
CopyVexCommon(
    _In_ PDETOUR_DISASM pDisasm,
    BYTE m,
    _In_ PBYTE pbDst,
    _In_ PBYTE pbSrc)
// m is first instead of last in the hopes of pbDst/pbSrc being
// passed along efficiently in the registers they were already in.
{
    pDisasm->bVex = TRUE;
    return CopyVexEvexCommon(pDisasm, m, pbDst, pbSrc, (BYTE)(pbSrc[-1] & 3), 0);
}

static
PBYTE
CopyVex3(
    _In_ PDETOUR_DISASM pDisasm,
    _In_opt_ REFCOPYENTRY pEntry,
    _In_ PBYTE pbDst,
    _In_ PBYTE pbSrc)
// 3 byte VEX prefix 0xC4
{
    UNREFERENCED_PARAMETER(pEntry);

#if defined(_X86_)
    if ((pbSrc[1] & 0xC0) != 0xC0)
    {
        REFCOPYENTRY ce = &g_rceCopyMap[eENTRY_CopyBytes2Mod]; /* C4 ceLES */
        return ce->pfCopy(pDisasm, ce, pbDst, pbSrc);
    }
#endif
    pbDst[0] = pbSrc[0];
    pbDst[1] = pbSrc[1];
    pbDst[2] = pbSrc[2];
#if defined(_AMD64_)
    pDisasm->bRaxOverride |= !!(pbSrc[2] & 0x80); // w in last byte, see CopyBytesRax
#else
    //
    // TODO
    //
    // Usually the VEX.W bit changes the size of a general purpose register and is ignored for 32bit.
    // Sometimes it is an opcode extension.
    // Look in the Intel manual, in the instruction-by-instruction reference, for ".W1",
    // without nearby wording saying it is ignored for 32bit.
    // For example: "VFMADD132PD/VFMADD213PD/VFMADD231PD Fused Multiply-Add of Packed Double-Precision Floating-Point Values".
    //
    // Then, go through each such case and determine if W0 vs. W1 affect the size of the instruction. Probably not.
    // Look for the same encoding but with "W1" changed to "W0".
    // Here is one such pairing:
    // VFMADD132PD/VFMADD213PD/VFMADD231PD Fused Multiply-Add of Packed Double-Precision Floating-Point Values
    //
    // VEX.DDS.128.66.0F38.W1 98 /r A V/V FMA Multiply packed double-precision floating-point values
    // from xmm0 and xmm2/mem, add to xmm1 and
    // put result in xmm0.
    // VFMADD132PD xmm0, xmm1, xmm2/m128
    //
    // VFMADD132PS/VFMADD213PS/VFMADD231PS Fused Multiply-Add of Packed Single-Precision Floating-Point Values
    // VEX.DDS.128.66.0F38.W0 98 /r A V/V FMA Multiply packed single-precision floating-point values
    // from xmm0 and xmm2/mem, add to xmm1 and put
    // result in xmm0.
    // VFMADD132PS xmm0, xmm1, xmm2/m128
    //
#endif
    return CopyVexCommon(pDisasm, pbSrc[1] & 0x1F, pbDst + 3, pbSrc + 3);
}

static
PBYTE
CopyVex2(
    _In_ PDETOUR_DISASM pDisasm,
    _In_opt_ REFCOPYENTRY pEntry,
    _In_ PBYTE pbDst,
    _In_ PBYTE pbSrc)
// 2 byte VEX prefix 0xC5
{
#if defined(_X86_)
    if ((pbSrc[1] & 0xC0) != 0xC0)
    {
        REFCOPYENTRY ce = &g_rceCopyMap[eENTRY_CopyBytes2Mod]; /* C5 ceLDS */
        return ce->pfCopy(pDisasm, ce, pbDst, pbSrc);
    }
#endif
    pbDst[0] = pbSrc[0];
    pbDst[1] = pbSrc[1];
    return CopyVexCommon(pDisasm, 1, pbDst + 2, pbSrc + 2);
}

static
PBYTE
CopyEvex(
    _In_ PDETOUR_DISASM pDisasm,
    _In_opt_ REFCOPYENTRY pEntry,
    _In_ PBYTE pbDst,
    _In_ PBYTE pbSrc)
// 62, 3 byte payload, x86 with implied prefixes like Vex
// for 32bit, mode 0xC0 else fallback to bound /r
{
    // NOTE: Intel and Wikipedia number these differently.
    // Intel says 0-2, Wikipedia says 1-3.

    BYTE const p0 = pbSrc[1];

#if defined(_X86_)
    if ((p0 & 0xC0) != 0xC0)
    {
        return CopyBytes(pDisasm, &g_rceCopyMap[eENTRY_CopyBytes2Mod], pbDst, pbSrc); /* 62 ceBound */
    }
#endif

    // This could also be handled by default in CopyVexEvexCommon
    // if 4u changed to 4|8.
    if (p0 & 8u)
        return Invalid(pDisasm, &g_rceCopyMap[eENTRY_Invalid], pbDst, pbSrc); /* 62 ceInvalid */

    BYTE const p1 = pbSrc[2];

    if ((p1 & 0x04) != 0x04)
        return Invalid(pDisasm, &g_rceCopyMap[eENTRY_Invalid], pbDst, pbSrc); /* 62 ceInvalid */

    // Copy 4 byte prefix.
    *(UNALIGNED ULONG*)pbDst = *(UNALIGNED ULONG*)pbSrc;

    pDisasm->bEvex = TRUE;

#if defined(_AMD64_)
    pDisasm->bRaxOverride |= !!(p1 & 0x80); // w
#endif

    return CopyVexEvexCommon(pDisasm, p0 & 3u, pbDst + 4, pbSrc + 4, p1 & 3u, p0 & 4u);
}

static
PBYTE
CopyXop(
    _In_ PDETOUR_DISASM pDisasm,
    _In_opt_ REFCOPYENTRY pEntry,
    _In_ PBYTE pbDst,
    _In_ PBYTE pbSrc)
/* 3 byte AMD XOP prefix 0x8F
byte0: 0x8F
byte1: RXBmmmmm
byte2: WvvvvLpp
byte3: opcode
mmmmm >= 8, else pop
mmmmm only otherwise defined for 8, 9, A.
pp is like VEX but only instructions with 0 are defined
*/
{
    UNREFERENCED_PARAMETER(pEntry);

    BYTE const m = (BYTE)(pbSrc[1] & 0x1F);
    ASSERT(m <= 10);
    switch (m)
    {
        case 8: // modrm with 8bit immediate
            return CopyBytes(pDisasm, &g_rceCopyMap[eENTRY_CopyBytesXop1], pbDst, pbSrc); /* 8F ceXop1 */

        case 9: // modrm with no immediate
            return CopyBytes(pDisasm, &g_rceCopyMap[eENTRY_CopyBytesXop], pbDst, pbSrc); /* 8F ceXop */

        case 10: // modrm with 32bit immediate
            return CopyBytes(pDisasm, &g_rceCopyMap[eENTRY_CopyBytesXop4], pbDst, pbSrc); /* 8F ceXop4 */

        default:
            return CopyBytes(pDisasm, &g_rceCopyMap[eENTRY_CopyBytes2Mod], pbDst, pbSrc); /* 8F cePop */
    }
}

#endif // defined(_AMD64_) || defined(_X86_)

#if defined(_ARM64_)

typedef struct _DETOUR_DISASM
{
    PBYTE   pbTarget;
    BYTE    rbScratchDst[128]; // matches or exceeds rbCode
} DETOUR_DISASM, *PDETOUR_DISASM;

static
VOID
detour_disasm_init(
    _Out_ PDETOUR_DISASM pDisasm)
{
    pDisasm->pbTarget = (PBYTE)DETOUR_INSTRUCTION_TARGET_NONE;
}

typedef
BYTE(*COPYFUNC)(
    _In_ PBYTE pbDst,
    _In_ PBYTE pbSrc);

#define c_LR        30          // The register number for the Link Register
#define c_SP        31          // The register number for the Stack Pointer
#define c_NOP       0xd503201f  // A nop instruction
#define c_BREAK     (0xd4200000 | (0xf000 << 5)) // A break instruction

//
// Problematic instructions:
//
// ADR     0ll10000 hhhhhhhh hhhhhhhh hhhddddd  & 0x9f000000 == 0x10000000  (l = low, h = high, d = Rd)
// ADRP    1ll10000 hhhhhhhh hhhhhhhh hhhddddd  & 0x9f000000 == 0x90000000  (l = low, h = high, d = Rd)
//
// B.cond  01010100 iiiiiiii iiiiiiii iii0cccc  & 0xff000010 == 0x54000000  (i = delta = SignExtend(imm19:00, 64), c = cond)
//
// B       000101ii iiiiiiii iiiiiiii iiiiiiii  & 0xfc000000 == 0x14000000  (i = delta = SignExtend(imm26:00, 64))
// BL      100101ii iiiiiiii iiiiiiii iiiiiiii  & 0xfc000000 == 0x94000000  (i = delta = SignExtend(imm26:00, 64))
//
// CBNZ    z0110101 iiiiiiii iiiiiiii iiittttt  & 0x7f000000 == 0x35000000  (z = size, i = delta = SignExtend(imm19:00, 64), t = Rt)
// CBZ     z0110100 iiiiiiii iiiiiiii iiittttt  & 0x7f000000 == 0x34000000  (z = size, i = delta = SignExtend(imm19:00, 64), t = Rt)
//
// LDR Wt  00011000 iiiiiiii iiiiiiii iiittttt  & 0xff000000 == 0x18000000  (i = SignExtend(imm19:00, 64), t = Rt)
// LDR Xt  01011000 iiiiiiii iiiiiiii iiittttt  & 0xff000000 == 0x58000000  (i = SignExtend(imm19:00, 64), t = Rt)
// LDRSW   10011000 iiiiiiii iiiiiiii iiittttt  & 0xff000000 == 0x98000000  (i = SignExtend(imm19:00, 64), t = Rt)
// PRFM    11011000 iiiiiiii iiiiiiii iiittttt  & 0xff000000 == 0xd8000000  (i = SignExtend(imm19:00, 64), t = Rt)
// LDR St  00011100 iiiiiiii iiiiiiii iiittttt  & 0xff000000 == 0x1c000000  (i = SignExtend(imm19:00, 64), t = Rt)
// LDR Dt  01011100 iiiiiiii iiiiiiii iiittttt  & 0xff000000 == 0x5c000000  (i = SignExtend(imm19:00, 64), t = Rt)
// LDR Qt  10011100 iiiiiiii iiiiiiii iiittttt  & 0xff000000 == 0x9c000000  (i = SignExtend(imm19:00, 64), t = Rt)
// LDR inv 11011100 iiiiiiii iiiiiiii iiittttt  & 0xff000000 == 0xdc000000  (i = SignExtend(imm19:00, 64), t = Rt)
//
// TBNZ    z0110111 bbbbbiii iiiiiiii iiittttt  & 0x7f000000 == 0x37000000  (z = size, b = bitnum, i = SignExtend(imm14:00, 64), t = Rt)
// TBZ     z0110110 bbbbbiii iiiiiiii iiittttt  & 0x7f000000 == 0x36000000  (z = size, b = bitnum, i = SignExtend(imm14:00, 64), t = Rt)
//

typedef union
{
    DWORD Assembled;
    struct
    {
        DWORD Rd : 5;       // Destination register
        DWORD Rn : 5;       // Source register
        DWORD Imm12 : 12;   // 12-bit immediate
        DWORD Shift : 2;    // shift (must be 0 or 1)
        DWORD Opcode1 : 7;  // Must be 0010001 == 0x11
        DWORD Size : 1;     // 0 = 32-bit, 1 = 64-bit
    } s;
} AddImm12;

static
DWORD
AddImm12_Assemble(
    DWORD size,
    DWORD rd,
    DWORD rn,
    ULONG imm,
    DWORD shift)
{
    AddImm12 temp;
    temp.s.Rd = rd;
    temp.s.Rn = rn;
    temp.s.Imm12 = imm & 0xfff;
    temp.s.Shift = shift;
    temp.s.Opcode1 = 0x11;
    temp.s.Size = size;
    return temp.Assembled;
}

static
DWORD
AddImm12_AssembleAdd32(
    DWORD rd,
    DWORD rn,
    ULONG imm,
    DWORD shift)
{
    return AddImm12_Assemble(0, rd, rn, imm, shift);
}

static
DWORD
AddImm12_AssembleAdd64(
    DWORD rd,
    DWORD rn,
    ULONG imm,
    DWORD shift)
{
    return AddImm12_Assemble(1, rd, rn, imm, shift);
}

typedef union
{
    DWORD Assembled;
    struct
    {
        DWORD Rd : 5;       // Destination register
        DWORD Imm19 : 19;   // 19-bit upper immediate
        DWORD Opcode1 : 5;  // Must be 10000 == 0x10
        DWORD Imm2 : 2;     // 2-bit lower immediate
        DWORD Type : 1;     // 0 = ADR, 1 = ADRP
    } s;
} Adr19;

inline
LONG
Adr19_Imm(
    Adr19* p)
{
    DWORD Imm = (p->s.Imm19 << 2) | p->s.Imm2;
    return (LONG)(Imm << 11) >> 11;
}

static
DWORD
Adr19_Assemble(
    DWORD type,
    DWORD rd,
    LONG delta)
{
    Adr19 temp;
    temp.s.Rd = rd;
    temp.s.Imm19 = (delta >> 2) & 0x7ffff;
    temp.s.Opcode1 = 0x10;
    temp.s.Imm2 = delta & 3;
    temp.s.Type = type;
    return temp.Assembled;
}

static
DWORD
Adr19_AssembleAdr(
    DWORD rd,
    LONG delta)
{
    return Adr19_Assemble(0, rd, delta);
}

static
DWORD
Adr19_AssembleAdrp(
    DWORD rd,
    LONG delta)
{
    return Adr19_Assemble(1, rd, delta);
}

typedef union
{
    DWORD Assembled;
    struct
    {
        DWORD Condition : 4;    // Condition
        DWORD Opcode1 : 1;      // Must be 0
        DWORD Imm19 : 19;       // 19-bit immediate
        DWORD Opcode2 : 8;      // Must be 01010100 == 0x54
    } s;
} Bcc19;

inline
LONG
Bcc19_Imm(
    Bcc19* p)
{
    return (LONG)(p->s.Imm19 << 13) >> 11;
}

static
DWORD
Bcc19_AssembleBcc(
    DWORD condition,
    LONG delta)
{
    Bcc19 temp;
    temp.s.Condition = condition;
    temp.s.Opcode1 = 0;
    temp.s.Imm19 = delta >> 2;
    temp.s.Opcode2 = 0x54;
    return temp.Assembled;
}

typedef union
{
    DWORD Assembled;
    struct
    {
        DWORD Imm26 : 26;   // 26-bit immediate
        DWORD Opcode1 : 5;  // Must be 00101 == 0x5
        DWORD Link : 1;     // 0 = B, 1 = BL
    } s;
} Branch26;

inline
LONG
Branch26_Imm(
    Branch26* p)
{
    return (LONG)(p->s.Imm26 << 6) >> 4;
}

static
DWORD
Branch26_Assemble(
    DWORD link,
    LONG delta)
{
    Branch26 temp;
    temp.s.Imm26 = delta >> 2;
    temp.s.Opcode1 = 0x5;
    temp.s.Link = link;
    return temp.Assembled;
}

static
DWORD
Branch26_AssembleB(
    LONG delta)
{
    return Branch26_Assemble(0, delta);
}

static
DWORD
Branch26_AssembleBl(
    LONG delta)
{
    return Branch26_Assemble(1, delta);
}

typedef union
{
    DWORD Assembled;
    struct
    {
        DWORD Opcode1 : 5;  // Must be 00000 == 0
        DWORD Rn : 5;       // Register number
        DWORD Opcode2 : 22; // Must be 1101011000011111000000 == 0x3587c0 for Br
                            //                                   0x358fc0 for Brl
    } s;
} Br;

static
DWORD
Br_Assemble(
    DWORD rn,
    BOOL link)
{
    Br temp;
    temp.s.Opcode1 = 0;
    temp.s.Rn = rn;
    temp.s.Opcode2 = 0x3587c0;
    if (link)
        temp.Assembled |= 0x00200000;
    return temp.Assembled;
}

static
DWORD
Br_AssembleBr(
    DWORD rn)
{
    return Br_Assemble(rn, FALSE);
}

static
DWORD
Br_AssembleBrl(
    DWORD rn)
{
    return Br_Assemble(rn, TRUE);
}

typedef union
{
    DWORD Assembled;
    struct
    {
        DWORD Rt : 5;       // Register to test
        DWORD Imm19 : 19;   // 19-bit immediate
        DWORD Nz : 1;       // 0 = CBZ, 1 = CBNZ
        DWORD Opcode1 : 6;  // Must be 011010 == 0x1a
        DWORD Size : 1;     // 0 = 32-bit, 1 = 64-bit
    } s;
} Cbz19;

inline
LONG
Cbz19_Imm(
    Cbz19* p)
{
    return (LONG)(p->s.Imm19 << 13) >> 11;
}

static
DWORD
Cbz19_Assemble(
    DWORD size,
    DWORD nz,
    DWORD rt,
    LONG delta)
{
    Cbz19 temp;
    temp.s.Rt = rt;
    temp.s.Imm19 = delta >> 2;
    temp.s.Nz = nz;
    temp.s.Opcode1 = 0x1a;
    temp.s.Size = size;
    return temp.Assembled;
}

typedef union
{
    DWORD Assembled;
    struct
    {
        DWORD Rt : 5;       // Destination register
        DWORD Imm19 : 19;   // 19-bit immediate
        DWORD Opcode1 : 2;  // Must be 0
        DWORD FpNeon : 1;   // 0 = LDR Wt/LDR Xt/LDRSW/PRFM, 1 = LDR St/LDR Dt/LDR Qt
        DWORD Opcode2 : 3;  // Must be 011 = 3
        DWORD Size : 2;     // 00 = LDR Wt/LDR St, 01 = LDR Xt/LDR Dt, 10 = LDRSW/LDR Qt, 11 = PRFM/invalid
    } s;
} LdrLit19;

inline
LONG
LdrLit19_Imm(
    LdrLit19* p)
{
    return (LONG)(p->s.Imm19 << 13) >> 11;
}

static
DWORD
LdrLit19_Assemble(
    DWORD size,
    DWORD fpneon,
    DWORD rt,
    LONG delta)
{
    LdrLit19 temp;
    temp.s.Rt = rt;
    temp.s.Imm19 = delta >> 2;
    temp.s.Opcode1 = 0;
    temp.s.FpNeon = fpneon;
    temp.s.Opcode2 = 3;
    temp.s.Size = size;
    return temp.Assembled;
}

typedef union
{
    DWORD Assembled;
    struct
    {
        DWORD Rt : 5;       // Destination register
        DWORD Rn : 5;       // Base register
        DWORD Imm12 : 12;   // 12-bit immediate
        DWORD Opcode1 : 1;  // Must be 1 == 1
        DWORD Opc : 1;      // Part of size
        DWORD Opcode2 : 6;  // Must be 111101 == 0x3d
        DWORD Size : 2;     // Size (0=8-bit, 1=16-bit, 2=32-bit, 3=64-bit, 4=128-bit)
    } s;
} LdrFpNeonImm9;

static
DWORD
LdrFpNeonImm9_Assemble(
    DWORD size,
    DWORD rt,
    DWORD rn,
    ULONG imm)
{
    LdrFpNeonImm9 temp;
    temp.s.Rt = rt;
    temp.s.Rn = rn;
    temp.s.Imm12 = imm;
    temp.s.Opcode1 = 1;
    temp.s.Opc = size >> 2;
    temp.s.Opcode2 = 0x3d;
    temp.s.Size = size & 3;
    return temp.Assembled;
}

typedef union
{
    DWORD Assembled;
    struct
    {
        DWORD Rd : 5;       // Destination register
        DWORD Imm16 : 16;   // Immediate
        DWORD Shift : 2;    // Shift amount (0=0, 1=16, 2=32, 3=48)
        DWORD Opcode : 6;   // Must be 100101 == 0x25
        DWORD Type : 2;     // 0 = MOVN, 1 = reserved, 2 = MOVZ, 3 = MOVK
        DWORD Size : 1;     // 0 = 32-bit, 1 = 64-bit
    } s;
} Mov16;

static
DWORD
Mov16_Assemble(
    DWORD size,
    DWORD type,
    DWORD rd,
    DWORD imm,
    DWORD shift)
{
    Mov16 temp;
    temp.s.Rd = rd;
    temp.s.Imm16 = imm;
    temp.s.Shift = shift;
    temp.s.Opcode = 0x25;
    temp.s.Type = type;
    temp.s.Size = size;
    return temp.Assembled;
}

static
DWORD
Mov16_AssembleMovn32(
    DWORD rd,
    DWORD imm,
    DWORD shift)
{
    return Mov16_Assemble(0, 0, rd, imm, shift);
}

static
DWORD
Mov16_AssembleMovn64(
    DWORD rd,
    DWORD imm,
    DWORD shift)
{
    return Mov16_Assemble(1, 0, rd, imm, shift);
}

static
DWORD
Mov16_AssembleMovz32(
    DWORD rd,
    DWORD imm,
    DWORD shift)
{
    return Mov16_Assemble(0, 2, rd, imm, shift);
}

static
DWORD
Mov16_AssembleMovz64(
    DWORD rd,
    DWORD imm,
    DWORD shift)
{
    return Mov16_Assemble(1, 2, rd, imm, shift);
}

static
DWORD
Mov16_AssembleMovk32(
    DWORD rd,
    DWORD imm,
    DWORD shift)
{
    return Mov16_Assemble(0, 3, rd, imm, shift);
}

static
DWORD
Mov16_AssembleMovk64(
    DWORD rd,
    DWORD imm,
    DWORD shift)
{
    return Mov16_Assemble(1, 3, rd, imm, shift);
}

typedef union
{
    DWORD Assembled;
    struct
    {
        DWORD Rt : 5;       // Register to test
        DWORD Imm14 : 14;   // 14-bit immediate
        DWORD Bit : 5;      // 5-bit index
        DWORD Nz : 1;       // 0 = TBZ, 1 = TBNZ
        DWORD Opcode1 : 6;  // Must be 011011 == 0x1b
        DWORD Size : 1;     // 0 = 32-bit, 1 = 64-bit
    } s;
} Tbz14;

inline
LONG
Tbz14_Imm(
    Tbz14* p)
{
    return (LONG)(p->s.Imm14 << 18) >> 16;
}

static
DWORD
Tbz14_Assemble(
    DWORD size,
    DWORD nz,
    DWORD rt,
    DWORD bit,
    LONG delta)
{
    Tbz14 temp;
    temp.s.Rt = rt;
    temp.s.Imm14 = delta >> 2;
    temp.s.Bit = bit;
    temp.s.Nz = nz;
    temp.s.Opcode1 = 0x1b;
    temp.s.Size = size;
    return temp.Assembled;
}

inline
ULONG
GetInstruction(
    _In_ BYTE* pSource)
{
    return *(PULONG)pSource;
}

static
PULONG
EmitInstruction(
    _In_ PULONG pDstInst,
    ULONG instruction)
{
    *pDstInst = instruction;
    return pDstInst + 1;
}

static
PULONG
EmitMovImmediate(
    PULONG pDstInst,
    BYTE rd,
    UINT64 immediate)
{
    DWORD piece[4];
    piece[3] = (DWORD)((immediate >> 48) & 0xffff);
    piece[2] = (DWORD)((immediate >> 32) & 0xffff);
    piece[1] = (DWORD)((immediate >> 16) & 0xffff);
    piece[0] = (DWORD)((immediate >> 0) & 0xffff);

    // special case: MOVN with 32-bit dest
    if (piece[3] == 0 && piece[2] == 0 && piece[1] == 0xffff)
    {
        pDstInst = EmitInstruction(pDstInst, Mov16_AssembleMovn32(rd, piece[0] ^ 0xffff, 0));
    }
    // MOVN/MOVZ with 64-bit dest
    else
    {
        int zero_pieces = (piece[3] == 0x0000) + (piece[2] == 0x0000) + (piece[1] == 0x0000) + (piece[0] == 0x0000);
        int ffff_pieces = (piece[3] == 0xffff) + (piece[2] == 0xffff) + (piece[1] == 0xffff) + (piece[0] == 0xffff);
        DWORD defaultPiece = (ffff_pieces > zero_pieces) ? 0xffff : 0x0000;
        BOOL first = TRUE;
        for (int pieceNum = 3; pieceNum >= 0; pieceNum--)
        {
            DWORD curPiece = piece[pieceNum];
            if (curPiece != defaultPiece || (pieceNum == 0 && first))
            {
                if (first)
                {
                    if (defaultPiece == 0xffff)
                    {
                        pDstInst = EmitInstruction(pDstInst, Mov16_AssembleMovn64(rd, curPiece ^ 0xffff, pieceNum));
                    } else
                    {
                        pDstInst = EmitInstruction(pDstInst, Mov16_AssembleMovz64(rd, curPiece, pieceNum));
                    }
                    first = FALSE;
                } else
                {
                    pDstInst = EmitInstruction(pDstInst, Mov16_AssembleMovk64(rd, curPiece, pieceNum));
                }
            }
        }
    }

    return pDstInst;
}

static
BYTE
PureCopy32(
    _In_ PBYTE pSource,
    _In_ PBYTE pDest)
{
    *(ULONG*)pDest = *(ULONG*)pSource;
    return sizeof(ULONG);
}

/////////////////////////////////////////////////////////// Disassembler Code.
//

static
BYTE
CopyAdr(
    BYTE* pSource,
    BYTE* pDest,
    ULONG instruction)
{
    Adr19 decoded = { .Assembled = instruction };
    PULONG pDstInst = (PULONG)(pDest);

    // ADR case
    if (decoded.s.Type == 0)
    {
        BYTE* pTarget = pSource + Adr19_Imm(&decoded);
        LONG64 delta = pTarget - pDest;
        LONG64 deltaPage = ((ULONG_PTR)pTarget >> 12) - ((ULONG_PTR)pDest >> 12);

        // output as ADR
        if (delta >= -(1 << 20) && delta < (1 << 20))
        {
            pDstInst = EmitInstruction(pDstInst, Adr19_AssembleAdr(decoded.s.Rd, (LONG)delta));
        }
        // output as ADRP; ADD
        else if (deltaPage >= -(1 << 20) && (deltaPage < (1 << 20)))
        {
            pDstInst = EmitInstruction(pDstInst, Adr19_AssembleAdrp(decoded.s.Rd, (LONG)deltaPage));
            pDstInst = EmitInstruction(pDstInst, AddImm12_AssembleAdd32(decoded.s.Rd,
                                                                        decoded.s.Rd,
                                                                        ((ULONG)(ULONG_PTR)pTarget) & 0xfff,
                                                                        0));
        }
        // output as immediate move
        else
        {
            pDstInst = EmitMovImmediate(pDstInst, (BYTE)decoded.s.Rd, (ULONG_PTR)pTarget);
        }
    }
    // ADRP case
    else
    {
        BYTE* pTarget = (BYTE*)((((ULONG_PTR)pSource >> 12) + Adr19_Imm(&decoded)) << 12);
        LONG64 deltaPage = ((ULONG_PTR)pTarget >> 12) - ((ULONG_PTR)pDest >> 12);

        // output as ADRP
        if (deltaPage >= -(1 << 20) && (deltaPage < (1 << 20)))
        {
            pDstInst = EmitInstruction(pDstInst, Adr19_AssembleAdrp(decoded.s.Rd, (LONG)deltaPage));
        }
        // output as immediate move
        else
        {
            pDstInst = EmitMovImmediate(pDstInst, (BYTE)decoded.s.Rd, (ULONG_PTR)pTarget);
        }
    }

    return (BYTE)((BYTE*)pDstInst - pDest);
}

static
BYTE
CopyBcc(
    _In_ PDETOUR_DISASM pDisasm,
    BYTE* pSource,
    BYTE* pDest,
    ULONG instruction)
{
    Bcc19 decoded = { .Assembled = instruction };
    PULONG pDstInst = (PULONG)(pDest);

    BYTE* pTarget = pSource + Bcc19_Imm(&decoded);
    pDisasm->pbTarget = pTarget;
    LONG64 delta = pTarget - pDest;
    LONG64 delta4 = pTarget - (pDest + 4);

    // output as BCC
    if (delta >= -(1 << 20) && delta < (1 << 20))
    {
        pDstInst = EmitInstruction(pDstInst, Bcc19_AssembleBcc(decoded.s.Condition, (LONG)delta));
    }
    // output as BCC <skip>; B
    else if (delta4 >= -(1 << 27) && (delta4 < (1 << 27)))
    {
        pDstInst = EmitInstruction(pDstInst, Bcc19_AssembleBcc(decoded.s.Condition ^ 1, 8));
        pDstInst = EmitInstruction(pDstInst, Branch26_AssembleB((LONG)delta4));
    }
    // output as MOV x17, Target; BCC <skip>; BR x17 (BIG assumption that x17 isn't being used for anything!!)
    else
    {
        pDstInst = EmitMovImmediate(pDstInst, 17, (ULONG_PTR)pTarget);
        pDstInst = EmitInstruction(pDstInst, Bcc19_AssembleBcc(decoded.s.Condition ^ 1, 8));
        pDstInst = EmitInstruction(pDstInst, Br_AssembleBr(17));
    }

    return (BYTE)((BYTE*)pDstInst - pDest);
}

static
BYTE
CopyB_or_Bl(
    _In_ PDETOUR_DISASM pDisasm,
    BYTE* pSource,
    BYTE* pDest,
    ULONG instruction,
    BOOL link)
{
    Branch26 decoded = { .Assembled = instruction };
    PULONG pDstInst = (PULONG)(pDest);

    BYTE* pTarget = pSource + Branch26_Imm(&decoded);
    pDisasm->pbTarget = pTarget;
    LONG64 delta = pTarget - pDest;

    // output as B or BRL
    if (delta >= -(1 << 27) && (delta < (1 << 27)))
    {
        pDstInst = EmitInstruction(pDstInst, Branch26_Assemble(link, (LONG)delta));
    }
    // output as MOV x17, Target; BR or BRL x17 (BIG assumption that x17 isn't being used for anything!!)
    else
    {
        pDstInst = EmitMovImmediate(pDstInst, 17, (ULONG_PTR)pTarget);
        pDstInst = EmitInstruction(pDstInst, Br_Assemble(17, link));
    }

    return (BYTE)((BYTE*)pDstInst - pDest);
}

static
BYTE
CopyB(
    _In_ PDETOUR_DISASM pDisasm,
    BYTE* pSource,
    BYTE* pDest,
    ULONG instruction)
{
    return CopyB_or_Bl(pDisasm, pSource, pDest, instruction, FALSE);
}

static
BYTE
CopyBl(
    _In_ PDETOUR_DISASM pDisasm,
    BYTE* pSource,
    BYTE* pDest,
    ULONG instruction)
{
    return CopyB_or_Bl(pDisasm, pSource, pDest, instruction, FALSE);
}

static
BYTE
CopyCbz(
    _In_ PDETOUR_DISASM pDisasm,
    BYTE* pSource,
    BYTE* pDest,
    ULONG instruction)
{
    Cbz19 decoded = { .Assembled = instruction };
    PULONG pDstInst = (PULONG)(pDest);

    BYTE* pTarget = pSource + Cbz19_Imm(&decoded);
    pDisasm->pbTarget = pTarget;
    LONG64 delta = pTarget - pDest;
    LONG64 delta4 = pTarget - (pDest + 4);

    // output as CBZ/NZ
    if (delta >= -(1 << 20) && delta < (1 << 20))
    {
        pDstInst = EmitInstruction(pDstInst, Cbz19_Assemble(decoded.s.Size, decoded.s.Nz, decoded.s.Rt, (LONG)delta));
    }
    // output as CBNZ/Z <skip>; B
    else if (delta4 >= -(1 << 27) && (delta4 < (1 << 27)))
    {
        pDstInst = EmitInstruction(pDstInst, Cbz19_Assemble(decoded.s.Size, decoded.s.Nz ^ 1, decoded.s.Rt, 8));
        pDstInst = EmitInstruction(pDstInst, Branch26_AssembleB((LONG)delta4));
    }
    // output as MOV x17, Target; CBNZ/Z <skip>; BR x17 (BIG assumption that x17 isn't being used for anything!!)
    else
    {
        pDstInst = EmitMovImmediate(pDstInst, 17, (ULONG_PTR)pTarget);
        pDstInst = EmitInstruction(pDstInst, Cbz19_Assemble(decoded.s.Size, decoded.s.Nz ^ 1, decoded.s.Rt, 8));
        pDstInst = EmitInstruction(pDstInst, Br_AssembleBr(17));
    }

    return (BYTE)((BYTE*)pDstInst - pDest);
}

static
BYTE
CopyTbz(
    _In_ PDETOUR_DISASM pDisasm,
    BYTE* pSource,
    BYTE* pDest,
    ULONG instruction)
{
    Tbz14 decoded = { .Assembled = instruction };
    PULONG pDstInst = (PULONG)(pDest);

    BYTE* pTarget = pSource + Tbz14_Imm(&decoded);
    pDisasm->pbTarget = pTarget;
    LONG64 delta = pTarget - pDest;
    LONG64 delta4 = pTarget - (pDest + 4);

    // output as TBZ/NZ
    if (delta >= -(1 << 13) && delta < (1 << 13))
    {
        pDstInst = EmitInstruction(pDstInst, Tbz14_Assemble(decoded.s.Size,
                                                            decoded.s.Nz,
                                                            decoded.s.Rt,
                                                            decoded.s.Bit,
                                                            (LONG)delta));
    }
    // output as TBNZ/Z <skip>; B
    else if (delta4 >= -(1 << 27) && (delta4 < (1 << 27)))
    {
        pDstInst = EmitInstruction(pDstInst, Tbz14_Assemble(decoded.s.Size, decoded.s.Nz ^ 1, decoded.s.Rt, decoded.s.Bit, 8));
        pDstInst = EmitInstruction(pDstInst, Branch26_AssembleB((LONG)delta4));
    }
    // output as MOV x17, Target; TBNZ/Z <skip>; BR x17 (BIG assumption that x17 isn't being used for anything!!)
    else
    {
        pDstInst = EmitMovImmediate(pDstInst, 17, (ULONG_PTR)pTarget);
        pDstInst = EmitInstruction(pDstInst, Tbz14_Assemble(decoded.s.Size, decoded.s.Nz ^ 1, decoded.s.Rt, decoded.s.Bit, 8));
        pDstInst = EmitInstruction(pDstInst, Br_AssembleBr(17));
    }

    return (BYTE)((BYTE*)pDstInst - pDest);
}

static
BYTE
CopyLdrLiteral(
    BYTE* pSource,
    BYTE* pDest,
    ULONG instruction)
{
    LdrLit19 decoded = { .Assembled = instruction };
    PULONG pDstInst = (PULONG)(pDest);

    BYTE* pTarget = pSource + LdrLit19_Imm(&decoded);
    LONG64 delta = pTarget - pDest;

    // output as LDR
    if (delta >= -(1 << 21) && delta < (1 << 21))
    {
        pDstInst = EmitInstruction(pDstInst, LdrLit19_Assemble(decoded.s.Size, decoded.s.FpNeon, decoded.s.Rt, (LONG)delta));
    }

    // output as move immediate
    else if (decoded.s.FpNeon == 0)
    {
        UINT64 value = 0;
        switch (decoded.s.Size)
        {
            case 0: value = *(ULONG*)pTarget; break;
            case 1: value = *(UINT64*)pTarget; break;
            case 2: value = *(LONG*)pTarget; break;
        }
        pDstInst = EmitMovImmediate(pDstInst, (BYTE)decoded.s.Rt, value);
    }
    // FP/NEON register: compute address in x17 and load from there (BIG assumption that x17 isn't being used for anything!!)
    else
    {
        pDstInst = EmitMovImmediate(pDstInst, 17, (ULONG_PTR)pTarget);
        pDstInst = EmitInstruction(pDstInst, LdrFpNeonImm9_Assemble(2 + decoded.s.Size, decoded.s.Rt, 17, 0));
    }

    return (BYTE)((BYTE*)pDstInst - pDest);
}

static
PBYTE
CopyInstruction(
    _In_ PDETOUR_DISASM pDisasm,
    _In_opt_ PBYTE pDst,
    _In_ PBYTE pSrc,
    PBYTE* ppTarget,
    LONG* plExtra)
{
    if (pDst == NULL)
    {
        pDst = pDisasm->rbScratchDst;
    }

    DWORD Instruction = GetInstruction(pSrc);

    ULONG CopiedSize;
    if ((Instruction & 0x1f000000) == 0x10000000)
    {
        CopiedSize = CopyAdr(pSrc, pDst, Instruction);
    } else if ((Instruction & 0xff000010) == 0x54000000)
    {
        CopiedSize = CopyBcc(pDisasm, pSrc, pDst, Instruction);
    } else if ((Instruction & 0x7c000000) == 0x14000000)
    {
        CopiedSize = CopyB_or_Bl(pDisasm, pSrc, pDst, Instruction, (Instruction & 0x80000000) != 0);
    } else if ((Instruction & 0x7e000000) == 0x34000000)
    {
        CopiedSize = CopyCbz(pDisasm, pSrc, pDst, Instruction);
    } else if ((Instruction & 0x7e000000) == 0x36000000)
    {
        CopiedSize = CopyTbz(pDisasm, pSrc, pDst, Instruction);
    } else if ((Instruction & 0x3b000000) == 0x18000000)
    {
        CopiedSize = CopyLdrLiteral(pSrc, pDst, Instruction);
    } else
    {
        CopiedSize = PureCopy32(pSrc, pDst);
    }

    // If the target is needed, store our target
    if (ppTarget)
    {
        *ppTarget = pDisasm->pbTarget;
    }
    if (plExtra)
    {
        *plExtra = CopiedSize - sizeof(DWORD);
    }

    return pSrc + 4;
}

#endif // defined(_ARM64_)

PVOID
NTAPI
SlimDetoursCopyInstruction(
    _In_opt_ PVOID pDst,
    _In_ PVOID pSrc,
    _Out_opt_ PVOID* ppTarget,
    _Out_opt_ LONG* plExtra)
{
    DETOUR_DISASM Disasm;

#if defined(_AMD64_) || defined(_X86_)
    detour_disasm_init(&Disasm, (PBYTE*)ppTarget, plExtra);
    return (PVOID)CopyInstruction(&Disasm, (PBYTE)pDst, (PBYTE)pSrc);
#elif defined(_ARM64_)
    detour_disasm_init(&Disasm);
    return (PVOID)CopyInstruction(&Disasm,
                                  (PBYTE)pDst,
                                  (PBYTE)pSrc,
                                  (PBYTE*)ppTarget,
                                  plExtra);
#else
    return NULL;
#endif
}
