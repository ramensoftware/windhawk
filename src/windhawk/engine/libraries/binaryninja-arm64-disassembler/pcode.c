#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "decode.h"
#include "pcode.h"

int BitCount(uint32_t x)
{
	int result = 0;
	while (x)
	{
		if (x & 1)
			result += 1;
		x >>= 1;
	}
	return result;
}

bool BFXPreferred(uint32_t sf, uint32_t uns, uint32_t imms, uint32_t immr)
{
	// must not match UBFIZ/SBFIX alias
	if (imms < immr)
		return false;

	// must not match LSR/ASR/LSL alias (imms == 31 or 63)
	if (imms == ((sf << 5) | 0x1F))
		return false;

	// must not match UXTx/SXTx alias
	if (immr == 0)
	{
		// must not match 32-bit UXT[BH] or SXT[BH]
		if (sf == 0 && (imms == 7 || imms == 15))
			return false;

		// must not match 64-bit SXT[BHW]
		if ((sf == 1 && uns == 0) && (imms == 7 || imms == 15 || imms == 31))
			return false;
	}

	// must be UBFX/SBFX alias
	return true;
}

uint64_t rotate_right(uint64_t x, unsigned width, unsigned amount)
{
	amount = amount % width;
	x = (x >> amount) | (x << (width - amount));
	x = (width < 64) ? x & (((uint64_t)1 << width) - 1) : x;
	return x;
}

DecodeBitMasks_ReturnType DecodeBitMasks(
    uint8_t /*bit*/ immN, uint8_t /*bit(6)*/ imms, uint8_t /*bit(6)*/ immr)
{
	int ones_nbits = 0;
	if (immN == 1)
		ones_nbits = 6;
	else if ((imms & 0x3E) == 0x3C)
		ones_nbits = 1;
	else if ((imms & 0x3C) == 0x38)
		ones_nbits = 2;
	else if ((imms & 0x38) == 0x30)
		ones_nbits = 3;
	else if ((imms & 0x30) == 0x20)
		ones_nbits = 4;
	else if ((imms & 0x20) == 0)
		ones_nbits = 5;
	// TODO else: return undefined

	/* set 1's in element */
	int ones_n = (imms & ((1 << ones_nbits) - 1)) + 1;
	uint64_t result = ((uint64_t)1 << ones_n) - 1;

	/* rotate element */
	int elem_width = 1 << ones_nbits;
	result = rotate_right(result, elem_width, immr);

	/* replicate element */
	while (elem_width < 64)
	{
		result = (result << elem_width) | result;
		elem_width *= 2;
	}

	DecodeBitMasks_ReturnType dbmrt;
	// TODO: do this right
	dbmrt.wmask = result;
	dbmrt.tmask = result;
	return dbmrt;
}

/* idea to abandon pseudocode and compute+compare actual bitmask
  is from NetBSD sys/arch/aarch64/aarch64/disasm.c */
bool MoveWidePreferred(uint32_t sf, uint32_t immN, uint32_t immS, uint32_t immR)
{
	uint32_t splat = (immN << 6) | immS;
	if (sf == 1 && !((splat & 0x40) == 0x40))
		return false;
	if (sf == 0 && !((splat & 0x60) == 0x00))
		return false;

	DecodeBitMasks_ReturnType dbmrt = DecodeBitMasks(sf, immS, immR);
	uint64_t imm = dbmrt.wmask;

	/* MOVZ check, at most 16 zeroes not across halfword (16-bit) boundary */
	if (sf == 0)
		imm &= 0xffffffff;
	if (((imm & 0xffffffffffff0000) == 0) || ((imm & 0xffffffff0000ffff) == 0) ||
	    ((imm & 0xffff0000ffffffff) == 0) || ((imm & 0x0000ffffffffffff) == 0))
		return true;

	/* MOVN check, at most 16 ones not across halfword (16-bit) boundary */
	imm = ~imm;
	if (sf == 0)
		imm &= 0xffffffff;
	if (((imm & 0xffffffffffff0000) == 0) || ((imm & 0xffffffff0000ffff) == 0) ||
	    ((imm & 0xffff0000ffffffff) == 0) || ((imm & 0x0000ffffffffffff) == 0))
		return true;

	return false;
}

int HighestSetBit(uint64_t x)
{
	for (int i = 63; i >= 0 && x; --i)
	{
		if (x & 0x8000000000000000)
			return i;
		x <<= 1;
	}

	return -1;
}

int LowestSetBit(uint64_t x)
{
	for (int i = 0; i < 64; ++i)
	{
		if (x & 1)
			return i;
		x >>= 1;
	}

	return -1;
}

bool SVEMoveMaskPreferred(uint32_t imm13)
{
	// TODO: populate this
	return true;
}

enum ShiftType DecodeRegExtend(uint8_t op)
{
	switch (op & 7)
	{
	case 0b000:
		return ShiftType_UXTB;
	case 0b001:
		return ShiftType_UXTH;
	case 0b010:
		return ShiftType_UXTW;
	case 0b011:
		return ShiftType_UXTX;
	case 0b100:
		return ShiftType_SXTB;
	case 0b101:
		return ShiftType_SXTH;
	case 0b110:
		return ShiftType_SXTW;
	case 0b111:
		return ShiftType_SXTX;
	default:
		return ShiftType_NONE;
	}
}

enum ShiftType DecodeShift(uint8_t op)
{
	switch (op & 3)
	{
	case 0b00:
		return ShiftType_LSL;
	case 0b01:
		return ShiftType_LSR;
	case 0b10:
		return ShiftType_ASR;
	case 0b11:
		return ShiftType_ROR;
	default:
		return ShiftType_NONE;
	}
}

enum SystemOp SysOp(uint32_t op1, uint32_t CRn, uint32_t CRm, uint32_t op2)
{
//	uint32_t tmp = (op1 << 11) | (CRn << 7) | (CRm << 3) | op2;
	uint32_t tmp = (((op1 & 7) << 11) | ((CRn & 0xF) << 7) | ((CRm & 0xF) << 3) | ((op2) & 7));

	switch (tmp)
	{
	case AT_OP_S1E1R:
	case AT_OP_S1E1W:
	case AT_OP_S1E0R:
	case AT_OP_S1E0W:
	case AT_OP_S1E1RP:
	case AT_OP_S1E1WP:
	case AT_OP_S1E1A:
	case AT_OP_S1E2R:
	case AT_OP_S1E2W:
	case AT_OP_S12E1R:
	case AT_OP_S12E1W:
	case AT_OP_S12E0R:
	case AT_OP_S12E0W:
	case AT_OP_S1E2A:
	case AT_OP_S1E3R:
	case AT_OP_S1E3W:
	case AT_OP_S1E3A:
		return Sys_AT;  // S1E1R
//	case 0b00001111000000:
//		return Sys_AT;  // S1E1R
//	case 0b10001111000000:
//		return Sys_AT;  // S1E2R
//	case 0b11001111000000:
//		return Sys_AT;  // S1E3R
//	case 0b00001111000001:
//		return Sys_AT;  // S1E1W
//	case 0b00001111001001:
//		return Sys_AT;  // S1E1WP
//	case 0b10001111000001:
//		return Sys_AT;  // S1E2W
//	case 0b11001111000001:
//		return Sys_AT;  // S1E3W
//	case 0b00001111000010:
//		return Sys_AT;  // S1E0R
//	case 0b00001111000011:
//		return Sys_AT;  // S1E0W
//	case 0b10001111000100:
//		return Sys_AT;  // S12E1R
//	case 0b10001111000101:
//		return Sys_AT;  // S12E1W
//	case 0b10001111000110:
//		return Sys_AT;  // S12E0R
//	case 0b10001111000111:
//		return Sys_AT;  // S12E0W
	case 0b01101110100001:
		return Sys_DC;  // ZVA
	case 0b00001110110001:
		return Sys_DC;  // IVAC
	case 0b00001110110010:
		return Sys_DC;  // ISW
	case 0b01101111010001:
		return Sys_DC;  // CVAC
	case 0b00001111010010:
		return Sys_DC;  // CSW
	case 0b01101111011001:
		return Sys_DC;  // CVAU
	case 0b01101111110001:
		return Sys_DC;  // CIVAC
	case 0b00001111110010:
		return Sys_DC;  // CISW
	case 0b01101111101001:
		return Sys_DC;  // CVADP
	case 0b00001110001000:
		return Sys_IC;  // IALLUIS
	case 0b00001110101000:
		return Sys_IC;  // IALLU
	case 0b01101110101001:
		return Sys_IC;  // IVAU
	case TLBI_VMALLE1OS:
	case TLBI_VAE1OS:
	case TLBI_ASIDE1OS:
	case TLBI_VAAE1OS:
	case TLBI_VALE1OS:
	case TLBI_VAALE1OS:
	case TLBI_RVAE1IS:
	case TLBI_RVAAE1IS:
	case TLBI_RVALE1IS:
	case TLBI_RVAALE1IS:
	case TLBI_VMALLE1IS:
	case TLBI_VAE1IS:
	case TLBI_ASIDE1IS:
	case TLBI_VAAE1IS:
	case TLBI_VALE1IS:
	case TLBI_VAALE1IS:
	case TLBI_RVAE1OS:
	case TLBI_RVAAE1OS:
	case TLBI_RVALE1OS:
	case TLBI_RVAALE1OS:
	case TLBI_RVAE1:
	case TLBI_RVAAE1:
	case TLBI_RVALE1:
	case TLBI_RVAALE1:
	case TLBI_VMALLE1:
	case TLBI_VAE1:
	case TLBI_ASIDE1:
	case TLBI_VAAE1:
	case TLBI_VALE1:
	case TLBI_VAALE1:
	case TLBI_VMALLE1OSNXS:
	case TLBI_VAE1OSNXS:
	case TLBI_ASIDE1OSNXS:
	case TLBI_VAAE1OSNXS:
	case TLBI_VALE1OSNXS:
	case TLBI_VAALE1OSNXS:
	case TLBI_RVAE1ISNXS:
	case TLBI_RVAAE1ISNXS:
	case TLBI_RVALE1ISNXS:
	case TLBI_RVAALE1ISNXS:
	case TLBI_VMALLE1ISNXS:
	case TLBI_VAE1ISNXS:
	case TLBI_ASIDE1ISNXS:
	case TLBI_VAAE1ISNXS:
	case TLBI_VALE1ISNXS:
	case TLBI_VAALE1ISNXS:
	case TLBI_RVAE1OSNXS:
	case TLBI_RVAAE1OSNXS:
	case TLBI_RVALE1OSNXS:
	case TLBI_RVAALE1OSNXS:
	case TLBI_RVAE1NXS:
	case TLBI_RVAAE1NXS:
	case TLBI_RVALE1NXS:
	case TLBI_RVAALE1NXS:
	case TLBI_VMALLE1NXS:
	case TLBI_VAE1NXS:
	case TLBI_ASIDE1NXS:
	case TLBI_VAAE1NXS:
	case TLBI_VALE1NXS:
	case TLBI_VAALE1NXS:
	case TLBI_IPAS2E1IS:
	case TLBI_RIPAS2E1IS:
	case TLBI_IPAS2LE1IS:
	case TLBI_RIPAS2LE1IS:
	case TLBI_ALLE2OS:
	case TLBI_VAE2OS:
	case TLBI_ALLE1OS:
	case TLBI_VALE2OS:
	case TLBI_VMALLS12E1OS:
	case TLBI_RVAE2IS:
	case TLBI_VMALLWS2E1IS:
	case TLBI_RVALE2IS:
	case TLBI_ALLE2IS:
	case TLBI_VAE2IS:
	case TLBI_ALLE1IS:
	case TLBI_VALE2IS:
	case TLBI_VMALLS12E1IS:
	case TLBI_IPAS2E1OS:
	case TLBI_IPAS2E1:
	case TLBI_RIPAS2E1:
	case TLBI_RIPAS2E1OS:
	case TLBI_IPAS2LE1OS:
	case TLBI_IPAS2LE1:
	case TLBI_RIPAS2LE1:
	case TLBI_RIPAS2LE1OS:
	case TLBI_RVAE2OS:
	case TLBI_VMALLWS2E1OS:
	case TLBI_RVALE2OS:
	case TLBI_RVAE2:
	case TLBI_VMALLWS2E1:
	case TLBI_RVALE2:
	case TLBI_ALLE2:
	case TLBI_VAE2:
	case TLBI_ALLE1:
	case TLBI_VALE2:
	case TLBI_VMALLS12E1:
	case TLBI_IPAS2E1ISNXS:
	case TLBI_RIPAS2E1ISNXS:
	case TLBI_IPAS2LE1ISNXS:
	case TLBI_RIPAS2LE1ISNXS:
	case TLBI_ALLE2OSNXS:
	case TLBI_VAE2OSNXS:
	case TLBI_ALLE1OSNXS:
	case TLBI_VALE2OSNXS:
	case TLBI_VMALLS12E1OSNXS:
	case TLBI_RVAE2ISNXS:
	case TLBI_VMALLWS2E1ISNXS:
	case TLBI_RVALE2ISNXS:
	case TLBI_ALLE2ISNXS:
	case TLBI_VAE2ISNXS:
	case TLBI_ALLE1ISNXS:
	case TLBI_VALE2ISNXS:
	case TLBI_VMALLS12E1ISNXS:
	case TLBI_IPAS2E1OSNXS:
	case TLBI_IPAS2E1NXS:
	case TLBI_RIPAS2E1NXS:
	case TLBI_RIPAS2E1OSNXS:
	case TLBI_IPAS2LE1OSNXS:
	case TLBI_IPAS2LE1NXS:
	case TLBI_RIPAS2LE1NXS:
	case TLBI_RIPAS2LE1OSNXS:
	case TLBI_RVAE2OSNXS:
	case TLBI_VMALLWS2E1OSNXS:
	case TLBI_RVALE2OSNXS:
	case TLBI_RVAE2NXS:
	case TLBI_VMALLWS2E1NXS:
	case TLBI_RVALE2NXS:
	case TLBI_ALLE2NXS:
	case TLBI_VAE2NXS:
	case TLBI_ALLE1NXS:
	case TLBI_VALE2NXS:
	case TLBI_VMALLS12E1NXS:
	case TLBI_ALLE3OS:
	case TLBI_VAE3OS:
	case TLBI_PAALLOS:
	case TLBI_VALE3OS:
	case TLBI_RVAE3IS:
	case TLBI_RVALE3IS:
	case TLBI_ALLE3IS:
	case TLBI_VAE3IS:
	case TLBI_VALE3IS:
	case TLBI_RPAOS:
	case TLBI_RPALOS:
	case TLBI_RVAE3OS:
	case TLBI_RVALE3OS:
	case TLBI_RVAE3:
	case TLBI_RVALE3:
	case TLBI_ALLE3:
	case TLBI_VAE3:
	case TLBI_PAALL:
	case TLBI_VALE3:
	case TLBI_ALLE3OSNXS:
	case TLBI_VAE3OSNXS:
	case TLBI_VALE3OSNXS:
	case TLBI_RVAE3ISNXS:
	case TLBI_RVALE3ISNXS:
	case TLBI_ALLE3ISNXS:
	case TLBI_VAE3ISNXS:
	case TLBI_VALE3ISNXS:
	case TLBI_RVAE3OSNXS:
	case TLBI_RVALE3OSNXS:
	case TLBI_RVAE3NXS:
	case TLBI_RVALE3NXS:
	case TLBI_ALLE3NXS:
	case TLBI_VAE3NXS:
	case TLBI_VALE3NXS:
		return Sys_TLBI;  // IPAS2E1IS
//	case 0b10010000000001:
//		return Sys_TLBI;  // IPAS2E1IS
//	case 0b10010000000101:
//		return Sys_TLBI;  // IPAS2LE1IS
//	case 0b00010000011000:
//		return Sys_TLBI;  // VMALLE1IS
//	case 0b10010000011000:
//		return Sys_TLBI;  // ALLE2IS
//	case 0b11010000011000:
//		return Sys_TLBI;  // ALLE3IS
//	case 0b00010000011001:
//		return Sys_TLBI;  // VAE1IS
//	case 0b10010000011001:
//		return Sys_TLBI;  // VAE2IS
//	case 0b11010000011001:
//		return Sys_TLBI;  // VAE3IS
//	case 0b00010000011010:
//		return Sys_TLBI;  // ASIDE1IS
//	case 0b00010000011011:
//		return Sys_TLBI;  // VAAE1IS
//	case 0b10010000011100:
//		return Sys_TLBI;  // ALLE1IS
//	case 0b00010000011101:
//		return Sys_TLBI;  // VALE1IS
//	case 0b10010000011101:
//		return Sys_TLBI;  // VALE2IS
//	case 0b11010000011101:
//		return Sys_TLBI;  // VALE3IS
//	case 0b10010000011110:
//		return Sys_TLBI;  // VMALLS12E1IS
//	case 0b00010000011111:
//		return Sys_TLBI;  // VAALE1IS
//	case 0b10010000100001:
//		return Sys_TLBI;  // IPAS2E1
//	case 0b10010000100101:
//		return Sys_TLBI;  // IPAS2LE1
//	case 0b00010000111000:
//		return Sys_TLBI;  // VMALLE1
//	case 0b10010000111000:
//		return Sys_TLBI;  // ALLE2
//	case 0b11010000111000:
//		return Sys_TLBI;  // ALLE3
//	case 0b00010000111001:
//		return Sys_TLBI;  // VAE1
//	case 0b10010000111001:
//		return Sys_TLBI;  // VAE2
//	case 0b11010000111001:
//		return Sys_TLBI;  // VAE3
//	case 0b00010000111010:
//		return Sys_TLBI;  // ASIDE1
//	case 0b00010000111011:
//		return Sys_TLBI;  // VAAE1
//	case 0b10010000111100:
//		return Sys_TLBI;  // ALLE1
//	case 0b00010000111101:
//		return Sys_TLBI;  // VALE1
//	case 0b10010000111101:
//		return Sys_TLBI;  // VALE2
//	case 0b11010000111101:
//		return Sys_TLBI;  // VALE3
//	case 0b10010000111110:
//		return Sys_TLBI;  // VMALLS12E1
//	case 0b00010000111111:
//		return Sys_TLBI;  // VAALE1
	default:
		return Sys_ERROR;
	}
}

uint32_t UInt(uint32_t foo)
{
	return foo;
}

uint32_t BitSlice(uint64_t foo, int hi, int lo)  // including the endpoints
{
	int width = hi - lo + 1;
	uint64_t mask = (1 << width) - 1;
	return (foo >> lo) & mask;
}

bool IsZero(uint64_t foo)
{
	return foo == 0;
}

bool IsOnes(uint64_t foo, int width)
{
	return foo == (1 << width) - 1;
}

/* Mozilla MPL
  TODO: evaluate license, possibly rewrite
  https://github.com/Siguza/iometa/blob/master/src/a64.c */
uint64_t Replicate(uint64_t val, uint8_t times, uint64_t width)
{
	// Fast path
	switch (times)
	{
	case 64:
		val |= val << width;
		width <<= 1;
	case 32:
		val |= val << width;
		width <<= 1;
	case 16:
		val |= val << width;
		width <<= 1;
	case 8:
		val |= val << width;
		width <<= 1;
	case 4:
		val |= val << width;
		width <<= 1;
	case 2:
		val |= val << width;
	case 1:
		return val;
	case 0:
		return 0;
	default:
		break;
	}
	// Slow path
	uint64_t orig = val;
	for (size_t i = 0; i < times; ++i)
	{
		val <<= width;
		val |= orig;
	}
	return val;
}

/* Mozilla MPL
  TODO: evaluate license, possibly rewrite
  https://github.com/Siguza/iometa/blob/master/src/a64.c */
uint64_t AdvSIMDExpandImm(uint8_t op, uint8_t cmode, uint64_t imm8)
{
	uint64_t imm64;
	switch ((cmode >> 1) & 0b111)
	{
	case 0b000:
		imm64 = Replicate(imm8, 2, 32);
		break;
	case 0b001:
		imm64 = Replicate(imm8 << 8, 2, 32);
		break;
	case 0b010:
		imm64 = Replicate(imm8 << 16, 2, 32);
		break;
	case 0b011:
		imm64 = Replicate(imm8 << 24, 2, 32);
		break;
	case 0b100:
		imm64 = Replicate(imm8, 4, 16);
		break;
	case 0b101:
		imm64 = Replicate(imm8 << 8, 4, 16);
		break;
	case 0b110:
		imm64 = Replicate(imm8 << (8 << (cmode & 0b1)), 2, 32);
		break;
	case 0b111:
		switch (((cmode & 0b1) << 1) | op)
		{
		case 0b00:
			imm64 = Replicate(imm8, 8, 8);
			break;
		case 0b01:
#if 0
					imm8a = Replicate((imm8 >> 7) & 0b1, 8, 1);
					imm8b = Replicate((imm8 >> 6) & 0b1, 8, 1);
					imm8c = Replicate((imm8 >> 5) & 0b1, 8, 1);
					imm8d = Replicate((imm8 >> 4) & 0b1, 8, 1);
					imm8e = Replicate((imm8 >> 3) & 0b1, 8, 1);
					imm8f = Replicate((imm8 >> 2) & 0b1, 8, 1);
					imm8g = Replicate((imm8 >> 1) & 0b1, 8, 1);
					imm8h = Replicate((imm8	 ) & 0b1, 8, 1);
					imm64 = (imm8a << 0x38) | (imm8b << 0x30) | (imm8c << 0x28) | (imm8d << 0x20) | (imm8e << 0x18) | (imm8f << 0x10) | (imm8g << 0x08) | imm8h;
#else
			imm64 = imm8 | (imm8 << (0x08 - 1)) | (imm8 << (0x10 - 2)) | (imm8 << (0x18 - 3)) |
			        (imm8 << (0x20 - 4)) | (imm8 << (0x28 - 5)) | (imm8 << (0x30 - 6)) |
			        (imm8 << (0x38 - 7));
			imm64 &= 0x0101010101010101;
			imm64 = Replicate(imm64, 8, 1);
#endif
			break;
		case 0b10:
			imm64 = Replicate((((imm8 & 0xc0) ^ 0x80) << 24) |
			                      (Replicate((imm8 >> 6) & 0b1, 5, 1) << 25) | ((imm8 & 0x3f) << 19),
			    2, 32);
			break;
		case 0b11:
			imm64 = (((imm8 & 0xc0) ^ 0x80) << 56) | (Replicate((imm8 >> 6) & 0b1, 8, 1) << 54) |
			        ((imm8 & 0x3f) << 48);
			break;
		}
		break;
	}
	return imm64;
}

bool BTypeCompatible_BTI(uint8_t hintcode, uint8_t pstate_btype)
{
	switch (hintcode & 3)
	{
	case 0b00:
		return false;
	case 0b01:
		return pstate_btype != 0b11;
	case 0b10:
		return pstate_btype != 0b10;
	case 0b11:
		return true;
	}

	return false; /* impossible, but appease compiler */
}

bool BTypeCompatible_PACIXSP()
{
	// TODO: determine if filling this in is necessary
	return true;
}

enum FPRounding FPDecodeRounding(uint8_t RMode)
{
	switch (RMode & 3)
	{
	case 0b00:
		return FPRounding_TIEEVEN;  // N
	case 0b01:
		return FPRounding_POSINF;  // P
	case 0b10:
		return FPRounding_NEGINF;  // M
	case 0b11:
		return FPRounding_ZERO;  // Z
	}

	return FPRounding_ERROR;
}

enum FPRounding FPRoundingMode(uint64_t fpcr)
{
	return FPDecodeRounding(FPCR_GET_RMode(fpcr));
}

bool HaltingAllowed(void)
{
	// TODO: determine if filling this in is necessary
	return true;
}

// AArch64.SystemAccessTrap
void SystemAccessTrap(uint32_t a, uint32_t b)
{
	// TODO: determine if filling this in is necessary
	while (0)
		;
}

void CheckSystemAccess(uint8_t a, uint8_t b, uint8_t c, uint8_t d, uint8_t e, uint8_t f, uint8_t g)
{
	// TODO: determine if filling this in is necessary
	while (0)
		;
}

// from LLVM
// TODO: check license, determine if rewrite needed
uint64_t VFPExpandImm(unsigned char byte, unsigned N)
{
	// assert(N == 32 || N == 64);

	uint64_t Result;
	unsigned bit6 = SLICE(byte, 6, 6);
	if (N == 32)
	{
		Result = SLICE(byte, 7, 7) << 31 | SLICE(byte, 5, 0) << 19;
		if (bit6)
			Result |= 0x1f << 25;
		else
			Result |= 0x1 << 30;
	}
	else
	{
		Result = (uint64_t)SLICE(byte, 7, 7) << 63 | (uint64_t)SLICE(byte, 5, 0) << 48;
		if (bit6)
			Result |= 0xffULL << 54;
		else
			Result |= 0x1ULL << 62;
	}

	// return APInt(N, Result);
	return Result;
}

bool EL2Enabled(void)
{
	// TODO: determine if filling this in is necessary
	return true;
}

bool ELUsingAArch32(uint8_t x)
{
	// TODO: determine if filling this in is necessary
	return true;
}

uint64_t FPOne(bool sign, int N)
{
	// width should be 16, 32, 64
	int E, F, exp;

	switch (N)
	{
	case 16:
		E = 5;
	case 32:
		E = 8;
	default:
		E = 11;
	}

	F = N - (E + 1);
	exp = BITMASK(E - 1) << 1;
	return (sign << (E - 1 + F)) | (exp << F);
}

uint64_t FPTwo(bool sign, int N)
{
	// width should be 16, 32, 64
	//int F;
	int E, exp;

	switch (N)
	{
	case 16:
		E = 5;
	case 32:
		E = 8;
	default:
		E = 11;
	}

	//F = N - (E + 1);
	exp = 1 << (E - 1);
	return (sign << E) | exp;
}

uint64_t FPPointFive(bool sign, int N)
{
	// width should be 16, 32, 64
	int E, F, exp;

	switch (N)
	{
	case 16:
		E = 5;
	case 32:
		E = 8;
	default:
		E = 11;
	}

	F = N - (E + 1);
	exp = BITMASK(E - 2) << 1;
	return (sign << (E - 2 + F)) | (exp << F);
}

uint64_t SignExtend(uint64_t x, int width)
{
	uint64_t result = -1;

	if (x & ((uint64_t)1 << (width - 1)))
	{
		result ^= (((uint64_t)1 << width) - 1);
		result |= x;
	}
	else
	{
		result = x;
	}

	return result;
}

enum Constraint ConstrainUnpredictable(enum Unpredictable u)
{
	switch (u)
	{
	case Unpredictable_VMSR:
		return Constraint_UNDEF;
	case Unpredictable_WBOVERLAPLD:
		return Constraint_WBSUPPRESS;  // return loaded value
	case Unpredictable_WBOVERLAPST:
		return Constraint_NONE;  // store pre-writeback value
	case Unpredictable_LDPOVERLAP:
		return Constraint_UNDEF;  // instruction is UNDEFINED
	case Unpredictable_BASEOVERLAP:
		return Constraint_NONE;  // use original address
	case Unpredictable_DATAOVERLAP:
		return Constraint_NONE;  // store original value
	case Unpredictable_DEVPAGE2:
		return Constraint_FAULT;  // take an alignment fault
	case Unpredictable_DEVICETAGSTORE:
		return Constraint_NONE;  // Do not take a fault
	case Unpredictable_INSTRDEVICE:
		return Constraint_NONE;  // Do not take a fault
	case Unpredictable_RESCPACR:
		return Constraint_TRUE;  // Map to UNKNOWN value
	case Unpredictable_RESMAIR:
		return Constraint_UNKNOWN;  // Map to UNKNOWN value
	case Unpredictable_RESTEXCB:
		return Constraint_UNKNOWN;  // Map to UNKNOWN value
	case Unpredictable_RESDACR:
		return Constraint_UNKNOWN;  // Map to UNKNOWN value
	case Unpredictable_RESPRRR:
		return Constraint_UNKNOWN;  // Map to UNKNOWN value
	case Unpredictable_RESVTCRS:
		return Constraint_UNKNOWN;  // Map to UNKNOWN value
	case Unpredictable_RESTnSZ:
		return Constraint_FORCE;  // Map to the limit value
	case Unpredictable_OORTnSZ:
		return Constraint_FORCE;  // Map to the limit value
	case Unpredictable_LARGEIPA:
		return Constraint_FORCE;  // Restrict the inputsize to the PAMax value
	case Unpredictable_ESRCONDPASS:
		return Constraint_FALSE;  // Report as "AL"
	case Unpredictable_ILZEROIT:
		return Constraint_FALSE;  // Do not zero PSTATE.IT
	case Unpredictable_ILZEROT:
		return Constraint_FALSE;  // Do not zero PSTATE.T
	case Unpredictable_BPVECTORCATCHPRI:
		return Constraint_TRUE;  // Debug Vector Catch: match on 2nd halfword
	case Unpredictable_VCMATCHHALF:
		return Constraint_FALSE;  // No match
	case Unpredictable_VCMATCHDAPA:
		return Constraint_FALSE;  // No match on Data Abort or Prefetch abort
	case Unpredictable_WPMASKANDBAS:
		return Constraint_FALSE;  // Watchpoint disabled
	case Unpredictable_WPBASCONTIGUOUS:
		return Constraint_FALSE;  // Watchpoint disabled
	case Unpredictable_RESWPMASK:
		return Constraint_DISABLED;  // Watchpoint disabled
	case Unpredictable_WPMASKEDBITS:
		return Constraint_FALSE;  // Watchpoint disabled
	case Unpredictable_RESBPWPCTRL:
		return Constraint_DISABLED;  // Breakpoint/watchpoint disabled
	case Unpredictable_BPNOTIMPL:
		return Constraint_DISABLED;  // Breakpoint disabled
	case Unpredictable_RESBPTYPE:
		return Constraint_DISABLED;  // Breakpoint disabled
	case Unpredictable_BPNOTCTXCMP:
		return Constraint_DISABLED;  // Breakpoint disabled
	case Unpredictable_BPMATCHHALF:
		return Constraint_FALSE;  // No match
	case Unpredictable_BPMISMATCHHALF:
		return Constraint_FALSE;  // No match
	case Unpredictable_RESTARTALIGNPC:
		return Constraint_FALSE;  // Do not force alignment
	case Unpredictable_RESTARTZEROUPPERPC:
		return Constraint_TRUE;  // Force zero extension
	case Unpredictable_ZEROUPPER:
		return Constraint_TRUE;  // zero top halves of X registers
	case Unpredictable_ERETZEROUPPERPC:
		return Constraint_TRUE;  // zero top half of PC
	case Unpredictable_A32FORCEALIGNPC:
		return Constraint_FALSE;  // Do not force alignment
	case Unpredictable_SMD:
		return Constraint_UNDEF;  // disabled SMC is Unallocated
	case Unpredictable_NONFAULT:
		return Constraint_FALSE;  // Speculation enabled
	case Unpredictable_SVEZEROUPPER:
		return Constraint_TRUE;  // zero top bits of Z registers
	case Unpredictable_SVELDNFDATA:
		return Constraint_TRUE;  // Load mem data in NF loads
	case Unpredictable_SVELDNFZERO:
		return Constraint_TRUE;  // Write zeros in NF loads
	case Unpredictable_CHECKSPNONEACTIVE:
		return Constraint_TRUE;  // Check SP alignment
	case Unpredictable_AFUPDATE:
		return Constraint_TRUE;
	case Unpredictable_IESBinDebug:
		return Constraint_TRUE;
	case Unpredictable_BADPMSFCR:
		return Constraint_TRUE;
	case Unpredictable_ZEROBTYPE:
		return Constraint_TRUE;  // Save BTYPE in SPSR_ELx/DPSR_EL0 as '00'
	case Unpredictable_CLEARERRITEZERO:
		return Constraint_FALSE;
	case Unpredictable_ALUEXCEPTIONRETURN:
		return Constraint_UNDEF;
	case Unpredictable_DBGxVR_RESS:
		return Constraint_FALSE;
	case Unpredictable_WFxTDEBUG:
		return Constraint_FALSE;  // WFxT in Debug state does not execute as a NOP
	case Unpredictable_LS64UNSUPPORTED:
		return Constraint_LIMITED_ATOMICITY;  //
	default:
		return Constraint_ERROR;
	}
}
