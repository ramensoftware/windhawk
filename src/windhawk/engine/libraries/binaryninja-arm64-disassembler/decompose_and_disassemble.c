#include "decompose_and_disassemble.h"

#include "decode.h"
#include "format.h"

int aarch64_decompose_and_disassemble(uint64_t address, uint32_t insword, char* result, size_t result_buf_sz)
{
	int rc;
	Instruction instr;
	memset(&instr, 0, sizeof(instr));

	rc = aarch64_decompose(insword, &instr, address);
	if (rc)
		return rc;

	rc = aarch64_disassemble(&instr, result, result_buf_sz);
	if (rc)
		return rc;

	return 0;
}
