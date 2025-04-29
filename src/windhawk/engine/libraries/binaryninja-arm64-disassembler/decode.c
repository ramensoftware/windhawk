#include "decode.h"
#include "feature_flags.h"

int decode_spec(context* ctx, Instruction* dec);        // from decode0.cpp
int decode_scratchpad(context* ctx, Instruction* dec);  // from decode_scratchpad.c

int aarch64_decompose(uint32_t instructionValue, Instruction* instr, uint64_t address)
{
	context ctx = {0};
	ctx.halted = 1;  // enable disassembly of exception instructions like DCPS1
	ctx.insword = instructionValue;
	ctx.address = address;
	ctx.features0 = ARCH_FEATURES_ALL;
	ctx.features1 = ARCH_FEATURES_ALL;
	ctx.EDSCR_HDE = 1;

	/* have the spec-generated code populate all the pcode variables */
	int rc = decode_spec(&ctx, instr);

	if (rc != DECODE_STATUS_OK)
	{
		/* exceptional cases where we accept a non-OK decode status */
		if (rc == DECODE_STATUS_END_OF_INSTRUCTION && instr->encoding == ENC_HINT_HM_HINTS)
		{
			while (0)
				;
		}
		/* no exception! fail! */
		else
			return rc;
	}

	/* if UDF encoding, return undefined */
	// if(instr->encoding == ENC_UDF_ONLY_PERM_UNDEF)
	//	return DECODE_STATUS_UNDEFINED;

	/* convert the pcode variables to list of operands, etc. */
	return decode_scratchpad(&ctx, instr);
}
