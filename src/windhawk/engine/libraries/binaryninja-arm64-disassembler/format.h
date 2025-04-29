#pragma once

#include <stdint.h>
#include <stdbool.h>

#include "decode.h"
#include "encodings_dec.h"
#include "encodings_fmt.h"
#include "operations.h"
#include "regs.h"
#include "sysregs_fmt_gen.h"

//-----------------------------------------------------------------------------
// disassembly function prototypes, return values
//-----------------------------------------------------------------------------

/* these get returned by the disassemble_instruction() function */
enum FailureCode {
	DISASM_SUCCESS=0,
	INVALID_ARGUMENTS,
	FAILED_TO_DISASSEMBLE_OPERAND,
	FAILED_TO_DISASSEMBLE_OPERATION,
	FAILED_TO_DISASSEMBLE_REGISTER,
	FAILED_TO_DECODE_INSTRUCTION,
	OUTPUT_BUFFER_TOO_SMALL,
	OPERAND_IS_NOT_REGISTER,
	NOT_MEMORY_OPERAND
};

#ifdef __cplusplus
extern "C" {
#endif

// get a text representation of the decomposed instruction
int aarch64_disassemble(Instruction *instruction, char *buf, size_t buf_sz);

// register (and related) to string
int get_register_full(enum Register, const InstructionOperand *, char *result);
const char *get_register_arrspec(enum Register, const InstructionOperand *);

// miscellany to string
const char *get_operation(const Instruction *instruction);
const char *get_shift(ShiftType shift);
const char *get_condition(Condition cond);
uint32_t get_implementation_specific(
		const InstructionOperand *operand,
		char *outBuffer,
		uint32_t outBufferSize);
const char *get_arrspec_str(ArrangementSpec arrspec);
const char *get_arrspec_str_truncated(ArrangementSpec arrspec);

#ifdef __cplusplus
}
#endif


