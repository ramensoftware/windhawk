#include "feature_flags.h"

#define INSWORD (ctx->insword)
#define UNDEFINED \
	{ \
		return DECODE_STATUS_UNDEFINED; \
	}
#define UNMATCHED \
	{ \
		return DECODE_STATUS_UNMATCHED; \
	}
#define RESERVED(X) \
	{ \
		return DECODE_STATUS_RESERVED; \
	}
#define UNALLOCATED(X) \
	{ \
		dec->encoding = (X); \
		return DECODE_STATUS_UNALLOCATED; \
	}
#define ENDOFINSTRUCTION \
	{ \
		return DECODE_STATUS_END_OF_INSTRUCTION; \
	}
#define SEE \
	{ \
		return DECODE_STATUS_LOST; \
	}
#define UNREACHABLE \
	{ \
		return DECODE_STATUS_UNREACHABLE; \
	}
/* do NOT return immediately! post-decode pcode might still need to run */
#define OK(X) \
	{ \
		instr->encoding = (X); \
		instr->operation = enc_to_oper(X); \
		rc = DECODE_STATUS_OK; \
	}
#define assert(X) \
	if (!(X)) \
	{ \
		return DECODE_STATUS_ASSERT_FAILED; \
	}

#define BITMASK(N)            (((uint64_t)1 << (N)) - 1)
#define SLICE(X, MSB, LSB)    (((X) >> (LSB)) & BITMASK((MSB) - (LSB) + 1)) /* get bits [MSB,LSB] */
#define CONCAT(A, B, B_WIDTH) (((A) << (B_WIDTH)) | (B))
#define NOT(X, X_WIDTH)       ((X) ^ BITMASK(X_WIDTH))

#define DecodeBitMasksCheckUndefined(N, imms) \
	if ((N == 0 && \
	        (imms == 0x3D || imms == 0x3B || imms == 0x37 || imms == 0x2F || imms == 0x1F)) || \
	    (N == 1 && imms == 0x3F)) \
	{ \
		return DECODE_STATUS_UNDEFINED; \
	}

#define UINT(x)          (unsigned int)(x)
#define SInt(X, X_WIDTH) SignExtend((X), (X_WIDTH))
#define INT(x)           (signed int)(x)
#define ZeroExtend(X, Y) (uint64_t)(X)
#define LSL(X, Y)        ((X) << (Y))

#define LOG2_TAG_GRANULE 4
#define TAG_GRANULE      (1 << LOG2_TAG_GRANULE)

/* pcode -> cpp booleans */
#define TRUE  true
#define FALSE false

/* these calls just check generated per-iform boolean variables */
#define EncodingLabeled32Bit() (encoding32)
#define EncodingLabeled64Bit() (encoding64)

// extras we find in spec tables, etc.
#define HaveTLBIOS()    (1)
#define HaveTLBIRANGE() (1)
#define HaveDCCVADP()   (1)
#define HaveDCPoP()     (1)

#define SetBTypeCompatible(X) ctx->BTypeCompatible = (X)
#define SetBTypeNext(X)       ctx->BTypeNext = (X)
#define Halted()              ctx->halted

enum SystemOp
{
	Sys_ERROR = -1,
	Sys_AT = 0,
	Sys_DC = 1,
	Sys_IC = 2,
	Sys_TLBI = 3,
	Sys_SYS = 4,
};

enum ReduceOp
{
	ReduceOp_ERROR = 0,
	ReduceOp_ADD,
	ReduceOp_FADD,
	ReduceOp_FMIN,
	ReduceOp_FMAX,
	ReduceOp_FMINNUM,
	ReduceOp_FMAXNUM,
};

enum LogicalOp
{
	LogicalOp_ERROR = 0,
	LogicalOp_AND,
	LogicalOp_EOR,
	LogicalOp_ORR
};

enum BranchType
{
	BranchType_ERROR = 0,
	BranchType_DIRCALL,    // Direct Branch with link
	BranchType_INDCALL,    // Indirect Branch with link
	BranchType_ERET,       // Exception return (indirect)
	BranchType_DBGEXIT,    // Exit from Debug state
	BranchType_RET,        // Indirect branch with function return hint
	BranchType_DIR,        // Direct branch
	BranchType_INDIR,      // Indirect branch
	BranchType_EXCEPTION,  // Exception entry
	BranchType_RESET,      // Reset
	BranchType_UNKNOWN     // Other
};

enum VBitOp
{
	VBitOp_ERROR = 0,
	VBitOp_VBIF,
	VBitOp_VBIT,
	VBitOp_VBSL,
	VBitOp_VEOR
};

enum SystemHintOp
{
	SystemHintOp_ERROR = 0,
	SystemHintOp_NOP,
	SystemHintOp_YIELD,
	SystemHintOp_WFE,
	SystemHintOp_WFI,
	SystemHintOp_SEV,
	SystemHintOp_SEVL,
	SystemHintOp_DGH,
	SystemHintOp_ESB,
	SystemHintOp_PSB,
	SystemHintOp_TSB,
	SystemHintOp_BTI,
	SystemHintOp_CSDB,
	SystemHintOp_WFET,
	SystemHintOp_WFIT,
};

enum ImmediateOp
{
	ImmediateOp_ERROR = 0,
	ImmediateOp_MOVI,
	ImmediateOp_MVNI,
	ImmediateOp_ORR,
	ImmediateOp_BIC
};

enum AccType
{
	AccType_ERROR = 0,
	AccType_ATOMICRW,
	AccType_ATOMIC,
	AccType_LIMITEDORDERED,
	AccType_ORDEREDATOMICRW,
	AccType_ORDEREDATOMIC,
	AccType_ORDERED
};

enum CompareOp
{
	CompareOp_ERROR = 0,
	CompareOp_EQ,
	CompareOp_GE,
	CompareOp_GT,
	CompareOp_LE,
	CompareOp_LT
};

enum Constraint
{
	Constraint_ERROR = 0,
	Constraint_DISABLED,
	Constraint_FALSE,
	Constraint_FAULT,
	Constraint_FORCE,
	Constraint_LIMITED_ATOMICITY,
	Constraint_NONE,
	Constraint_NOP,
	Constraint_TRUE,
	Constraint_UNDEF,
	Constraint_UNKNOWN,
	Constraint_WBSUPPRESS,
};

enum CountOp
{
	CountOp_ERROR = 0,
	CountOp_CLS,
	CountOp_CLZ
};

enum DSBAlias
{
	DSBAlias_DSB = 0,
	DSBAlias_SSBB,
	DSBAlias_PSSBB
};

enum MBReqDomain
{
	MBReqDomain_ERROR = 0,
	MBReqDomain_Nonshareable,
	MBReqDomain_InnerShareable,
	MBReqDomain_OuterShareable,
	MBReqDomain_FullSystem
};

enum MBReqTypes
{
	MBReqTypes_ERROR = 0,
	MBReqTypes_Reads,
	MBReqTypes_Writes,
	MBReqTypes_All
};

enum FPUnaryOp
{
	FPUnaryOp_ERROR = 0,
	FPUnaryOp_ABS,
	FPUnaryOp_MOV,
	FPUnaryOp_NEG,
	FPUnaryOp_SQRT
};

enum FPConvOp
{
	FPConvOp_ERROR = 0,
	FPConvOp_CVT_FtoI,
	FPConvOp_CVT_ItoF,
	FPConvOp_MOV_FtoI,
	FPConvOp_MOV_ItoF,
	FPConvOp_CVT_FtoI_JS
};

enum FPMaxMinOp
{
	FPMaxMinOp_ERROR = 0,
	FPMaxMinOp_MAX,
	FPMaxMinOp_MIN,
	FPMaxMinOp_MAXNUM,
	FPMaxMinOp_MINNUM
};

enum FPRounding
{
	FPRounding_ERROR = 0,
	FPRounding_TIEEVEN,
	FPRounding_POSINF,
	FPRounding_NEGINF,
	FPRounding_ZERO,
	FPRounding_TIEAWAY,
	FPRounding_ODD
};

enum MemAtomicOp
{
	MemAtomicOp_ERROR = 0,
	MemAtomicOp_ADD,
	MemAtomicOp_BIC,
	MemAtomicOp_EOR,
	MemAtomicOp_ORR,
	MemAtomicOp_SMAX,
	MemAtomicOp_SMIN,
	MemAtomicOp_UMAX,
	MemAtomicOp_UMIN,
	MemAtomicOp_SWP
};

enum MemOp
{
	MemOp_ERROR = 0,
	MemOp_LOAD,
	MemOp_STORE,
	MemOp_PREFETCH
};

enum MoveWideOp
{
	MoveWideOp_ERROR = 0,
	MoveWideOp_N,
	MoveWideOp_Z,
	MoveWideOp_K
};

enum PSTATEField
{
	PSTATEField_ERROR = 0,
	PSTATEField_DAIFSet,
	PSTATEField_DAIFClr,
	PSTATEField_PAN,  // Armv8.1
	PSTATEField_UAO,  // Armv8.2
	PSTATEField_DIT,  // Armv8.4
	PSTATEField_SSBS,
	PSTATEField_TCO,  // Armv8.5
	PSTATEField_SP,
	PSTATEField_SVCRZA,
	PSTATEField_SVCRSM,
	PSTATEField_SVCRSMZA
};

enum SVECmp
{
	Cmp_ERROR = -1,
	Cmp_EQ,
	Cmp_NE,
	Cmp_GE,
	Cmp_GT,
	Cmp_LT,
	Cmp_LE,
	Cmp_UN
};

enum PrefetchHint
{
	Prefetch_ERROR = -1,
	Prefetch_READ,
	Prefetch_WRITE,
	Prefetch_EXEC
};

enum Unpredictable
{
	Unpredictable_ERROR = -1,
	Unpredictable_VMSR,
	Unpredictable_WBOVERLAPLD,
	Unpredictable_WBOVERLAPST,
	Unpredictable_LDPOVERLAP,
	Unpredictable_BASEOVERLAP,
	Unpredictable_DATAOVERLAP,
	Unpredictable_DEVPAGE2,
	Unpredictable_DEVICETAGSTORE,
	Unpredictable_INSTRDEVICE,
	Unpredictable_RESCPACR,
	Unpredictable_RESMAIR,
	Unpredictable_RESTEXCB,
	Unpredictable_RESDACR,
	Unpredictable_RESPRRR,
	Unpredictable_RESVTCRS,
	Unpredictable_RESTnSZ,
	Unpredictable_OORTnSZ,
	Unpredictable_LARGEIPA,
	Unpredictable_ESRCONDPASS,
	Unpredictable_ILZEROIT,
	Unpredictable_ILZEROT,
	Unpredictable_BPVECTORCATCHPRI,
	Unpredictable_VCMATCHHALF,
	Unpredictable_VCMATCHDAPA,
	Unpredictable_WPMASKANDBAS,
	Unpredictable_WPBASCONTIGUOUS,
	Unpredictable_RESWPMASK,
	Unpredictable_WPMASKEDBITS,
	Unpredictable_RESBPWPCTRL,
	Unpredictable_BPNOTIMPL,
	Unpredictable_RESBPTYPE,
	Unpredictable_BPNOTCTXCMP,
	Unpredictable_BPMATCHHALF,
	Unpredictable_BPMISMATCHHALF,
	Unpredictable_RESTARTALIGNPC,
	Unpredictable_RESTARTZEROUPPERPC,
	Unpredictable_ZEROUPPER,
	Unpredictable_ERETZEROUPPERPC,
	Unpredictable_A32FORCEALIGNPC,
	Unpredictable_SMD,
	Unpredictable_NONFAULT,
	Unpredictable_SVEZEROUPPER,
	Unpredictable_SVELDNFDATA,
	Unpredictable_SVELDNFZERO,
	Unpredictable_CHECKSPNONEACTIVE,
	Unpredictable_AFUPDATE,     // AF update for alignment or permission fault,
	Unpredictable_IESBinDebug,  // Use SCTLR[].IESB in Debug state,
	Unpredictable_BADPMSFCR,    // Bad settings for PMSFCR_EL1/PMSEVFR_EL1/PMSLATFR_EL1,
	Unpredictable_ZEROBTYPE,
	Unpredictable_CLEARERRITEZERO,  // Clearing sticky errors when instruction in flight,
	Unpredictable_ALUEXCEPTIONRETURN,
	Unpredictable_DBGxVR_RESS,
	Unpredictable_WFxTDEBUG,
	Unpredictable_LS64UNSUPPORTED,
};

typedef struct DecodeBitMasks_ReturnType_
{
	uint64_t wmask;
	uint64_t tmask;
} DecodeBitMasks_ReturnType;

int HighestSetBit(uint64_t x);
int LowestSetBit(uint64_t x);

bool BFXPreferred(uint32_t sf, uint32_t uns, uint32_t imms, uint32_t immr);
int BitCount(uint32_t x);
DecodeBitMasks_ReturnType DecodeBitMasks(
    uint8_t /*bit*/ immN, uint8_t /*bit(6)*/ imms, uint8_t /*bit(6)*/ immr);

enum Constraint ConstrainUnpredictable(enum Unpredictable);
bool MoveWidePreferred(uint32_t sf, uint32_t immN, uint32_t imms, uint32_t immr);
bool SVEMoveMaskPreferred(uint32_t imm13);
enum ShiftType DecodeRegExtend(uint8_t op);
enum ShiftType DecodeShift(uint8_t op);
enum SystemOp SysOp(uint32_t op1, uint32_t CRn, uint32_t CRm, uint32_t op2);
uint32_t UInt(uint32_t);
uint32_t BitSlice(uint64_t, int hi, int lo);  // including the endpoints
bool IsZero(uint64_t foo);
bool IsOnes(uint64_t foo, int width);
uint64_t Replicate(uint64_t val, uint8_t times, uint64_t width);
uint64_t AdvSIMDExpandImm(uint8_t op, uint8_t cmode, uint64_t imm8);

bool BTypeCompatible_BTI(uint8_t hintcode, uint8_t pstate_btype);
bool BTypeCompatible_PACIXSP(void);

enum FPRounding FPDecodeRounding(uint8_t RMode);
enum FPRounding FPRoundingMode(uint64_t fpcr);

bool HaltingAllowed(void);
void SystemAccessTrap(uint32_t a, uint32_t b);
void CheckSystemAccess(uint8_t, uint8_t, uint8_t, uint8_t, uint8_t, uint8_t, uint8_t);

uint64_t VFPExpandImm(uint8_t imm8, unsigned width);

#define EL0 0
#define EL1 1
#define EL2 2
#define EL3 3
bool EL2Enabled(void);
bool ELUsingAArch32(uint8_t);

uint64_t FPOne(bool sign, int width);
uint64_t FPTwo(bool sign, int width);
uint64_t FPPointFive(bool sign, int width);

uint64_t SignExtend(uint64_t x, int width);
