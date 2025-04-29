#include <stdbool.h>
#include <string.h>
#include <stdio.h>
#include <stdint.h>
#include <inttypes.h>

#include "format.h"
#include "regs.h"
#include "pcode.h"

const char *get_arrspec_str(ArrangementSpec arrspec)
{
	switch(arrspec) {
		case ARRSPEC_FULL: return ".1q";
		case ARRSPEC_2DOUBLES: return ".2d";
		case ARRSPEC_4SINGLES: return ".4s";
		case ARRSPEC_8HALVES: return ".8h";
		case ARRSPEC_16BYTES: return ".16b";
		case ARRSPEC_1DOUBLE: return ".1d";
		case ARRSPEC_2SINGLES: return ".2s";
		case ARRSPEC_4HALVES: return ".4h";
		case ARRSPEC_8BYTES: return ".8b";
		case ARRSPEC_1SINGLE: return ".1s";
		case ARRSPEC_2HALVES: return ".2h";
		case ARRSPEC_4BYTES: return ".4b";
		case ARRSPEC_1HALF: return ".1h";
		case ARRSPEC_1BYTE: return ".1b";
		default: return "";
	}
}

const char *get_arrspec_str_truncated(ArrangementSpec arrspec)
{
	switch(arrspec) {
		case ARRSPEC_FULL: return ".q";
		case ARRSPEC_2DOUBLES: return ".d";
		case ARRSPEC_4SINGLES: return ".s";
		case ARRSPEC_8HALVES: return ".h";
		case ARRSPEC_16BYTES: return ".b";
		case ARRSPEC_1DOUBLE: return ".d";
		case ARRSPEC_2SINGLES: return ".s";
		case ARRSPEC_4HALVES: return ".h";
		case ARRSPEC_8BYTES: return ".b";
		case ARRSPEC_1SINGLE: return ".s";
		case ARRSPEC_2HALVES: return ".h";
		case ARRSPEC_4BYTES: return ".4b"; // not an error, UDOT_asimdelem_D and SDOT_asimdelem_D use this
		case ARRSPEC_1HALF: return ".h";
		case ARRSPEC_1BYTE: return ".b";
		default: return "";
	}
}

const char *get_register_arrspec(Register reg, const InstructionOperand *operand)
{
	if(operand->arrSpec == ARRSPEC_NONE)
		return "";

	bool is_simd = reg >= REG_V0 && reg <= REG_V31;
	bool is_sve = reg >= REG_Z0 && reg <= REG_Z31;
	bool is_pred = reg >= REG_P0 && reg <= REG_P31;

	if(!is_simd && !is_sve && !is_pred)
		return "";

	if(operand->laneUsed || is_sve || is_pred)
		return get_arrspec_str_truncated(operand->arrSpec);

	return get_arrspec_str(operand->arrSpec);
}

int get_register_full(Register reg, const InstructionOperand *operand, char *result)
{
	strcpy(result, get_register_name(reg));
	if(result[0] == '\0')
		return -1;

	strcat(result, get_register_arrspec(reg, operand));

	return 0;
}

//-----------------------------------------------------------------------------
// miscellany to string
//-----------------------------------------------------------------------------

uint32_t get_implementation_specific(const InstructionOperand *operand, char *outBuffer, uint32_t outBufferSize)
{
	return snprintf(outBuffer,
			outBufferSize,
			"s%d_%d_c%d_c%d_%d",
			operand->implspec[0],
			operand->implspec[1],
			operand->implspec[2],
			operand->implspec[3],
			operand->implspec[4]) >= outBufferSize;
}

const char *get_operation(const Instruction *inst)
{
	return operation_to_str(inst->operation);
}

static const char *ConditionString[] = {
	"eq", "ne", "cs", "cc",
	"mi", "pl", "vs", "vc",
	"hi", "ls", "ge", "lt",
	"gt", "le", "al", "nv"
};

const char *get_condition(Condition cond)
{
	if (cond < 0 || cond >= END_CONDITION)
		return NULL;

	return ConditionString[cond];
}

static const char *ShiftString[] = {
	"NONE", "lsl", "lsr", "asr",
	"ror",  "uxtw", "sxtw", "sxtx",
	"uxtx", "sxtb", "sxth", "uxth",
	"uxtb", "msl"
};

const char *get_shift(ShiftType shift)
{
	if (shift <= ShiftType_NONE || shift >= ShiftType_END)
		return NULL;

	return ShiftString[shift];
}

//-----------------------------------------------------------------------------
// operand processing helpers
//-----------------------------------------------------------------------------

static inline uint32_t get_shifted_register(
	const InstructionOperand *operand,
	uint32_t registerNumber,
	char *outBuffer,
	uint32_t outBufferSize)
{
	char immBuff[32] = {0};
	char shiftBuff[64] = {0};

	char reg[16];
	if(get_register_full(operand->reg[registerNumber], operand, reg))
		return FAILED_TO_DISASSEMBLE_REGISTER;

	if (operand->shiftType != ShiftType_NONE)
	{
		if (operand->shiftValueUsed != 0)
		{
			if (snprintf(immBuff, sizeof(immBuff), " #%#x", operand->shiftValue) >= sizeof(immBuff))
			{
				return FAILED_TO_DISASSEMBLE_REGISTER;
			}
		}
		const char *shiftStr = get_shift(operand->shiftType);
		if (shiftStr == NULL)
			return FAILED_TO_DISASSEMBLE_OPERAND;
		snprintf(
				shiftBuff,
				sizeof(shiftBuff),
				", %s%s",
				shiftStr,
				immBuff);
	}
	if (snprintf(outBuffer, outBufferSize, "%s%s", reg, shiftBuff) < 0)
		return FAILED_TO_DISASSEMBLE_REGISTER;
	return DISASM_SUCCESS;
}

uint32_t get_memory_operand(
	const InstructionOperand *operand,
	char *outBuffer,
	uint32_t outBufferSize)
{
	char immBuff[64]= {0};
	char extendBuff[48] = {0};
	char paramBuff[32] = {0};

	char reg0[16]={'\0'}, reg1[16]={'\0'};
	if(get_register_full(operand->reg[0], operand, reg0))
		return FAILED_TO_DISASSEMBLE_REGISTER;

	const char *sign = "";
	int64_t imm = operand->immediate;
	if (operand->signedImm && (int64_t)imm < 0)
	{
		sign = "-";
		imm = -imm;
	}

	switch (operand->operandClass)
	{
		case MEM_REG:
			if (snprintf(outBuffer, outBufferSize, "[%s]", reg0) >= outBufferSize)
				return FAILED_TO_DISASSEMBLE_OPERAND;
			break;

		case MEM_PRE_IDX:
			if (snprintf(outBuffer, outBufferSize, "[%s, #%s%#" PRIx64 "]!", reg0, sign, (uint64_t)imm) >= outBufferSize)
				return FAILED_TO_DISASSEMBLE_OPERAND;
			break;

		case MEM_POST_IDX: // [<reg>], <reg|imm>
			if (operand->reg[1] != REG_NONE) {
				if(get_register_full((Register)operand->reg[1], operand, reg1))
					return FAILED_TO_DISASSEMBLE_REGISTER;

				snprintf(paramBuff, sizeof(paramBuff), ", %s", reg1);
			}
			else if (snprintf(paramBuff, sizeof(paramBuff), ", #%s%#" PRIx64, sign, (uint64_t)imm) >= sizeof(paramBuff))
				return FAILED_TO_DISASSEMBLE_OPERAND;

			if (snprintf(outBuffer, outBufferSize, "[%s]%s", reg0, paramBuff) >= outBufferSize)
				return FAILED_TO_DISASSEMBLE_OPERAND;

			break;

		case MEM_OFFSET: // [<reg> optional(imm)]
			if (operand->immediate != 0) {
				const char *mul_vl = operand->mul_vl ? ", mul vl" : "";
				if(snprintf(immBuff, sizeof(immBuff), ", #%s%#" PRIx64 "%s", sign, (uint64_t)imm, mul_vl) >= sizeof(immBuff)) {
					return FAILED_TO_DISASSEMBLE_OPERAND;
				}
			}

			if (snprintf(outBuffer, outBufferSize, "[%s%s]", reg0, immBuff) >= outBufferSize)
				return FAILED_TO_DISASSEMBLE_OPERAND;
			break;

		case MEM_EXTENDED:
			if(get_register_full(operand->reg[1], operand, reg1))
				return FAILED_TO_DISASSEMBLE_REGISTER;

			if (reg0[0] == '\0' || reg1[0] == '\0') {
				return FAILED_TO_DISASSEMBLE_OPERAND;
			}

			// immBuff, like "#0x0"
			if (operand->shiftValueUsed)
				if(snprintf(immBuff, sizeof(immBuff), " #%#x", operand->shiftValue) >= sizeof(immBuff))
					return FAILED_TO_DISASSEMBLE_OPERAND;

			// extendBuff, like "lsl #0x0"
			if (operand->shiftType != ShiftType_NONE)
			{
				if (snprintf(extendBuff, sizeof(extendBuff), ", %s%s",
							get_shift(operand->shiftType), immBuff) >= sizeof(extendBuff))
				{
					return FAILED_TO_DISASSEMBLE_OPERAND;
				}
			}

			// together, like "[x24, x30, lsl #0x0]"
			if (snprintf(outBuffer, outBufferSize, "[%s, %s%s]", reg0, reg1, extendBuff) >= outBufferSize)
				return FAILED_TO_DISASSEMBLE_OPERAND;

			break;
		default:
			return NOT_MEMORY_OPERAND;
	}
	return DISASM_SUCCESS;
}

uint32_t get_register(const InstructionOperand *operand, uint32_t registerNumber, char *outBuffer, uint32_t outBufferSize)
{
	/* 1) handle system registers */
	if(operand->operandClass == SYS_REG)
	{
		if (snprintf(outBuffer, outBufferSize, "%s",
			get_system_register_name(operand->sysreg)) >= outBufferSize)
			return FAILED_TO_DISASSEMBLE_REGISTER;
		return 0;
	}

	if(operand->operandClass != REG && operand->operandClass != MULTI_REG)
		return OPERAND_IS_NOT_REGISTER;

	/* 2) handle shifted registers */
	if (operand->shiftType != ShiftType_NONE)
	{
		return get_shifted_register(operand, registerNumber, outBuffer, outBufferSize);
	}

	char reg_buf[16];
	if(get_register_full(operand->reg[registerNumber], operand, reg_buf))
		return FAILED_TO_DISASSEMBLE_REGISTER;

	/* 3) handle predicate registers */
	if(operand->operandClass == REG && operand->pred_qual && operand->reg[0] >= REG_P0 && operand->reg[0] <= REG_P31)
	{
		if(snprintf(outBuffer, outBufferSize, "%s/%c", reg_buf, operand->pred_qual) >= outBufferSize)
			return FAILED_TO_DISASSEMBLE_REGISTER;
		return 0;
	}

	/* 4) handle other registers */
	char index[32] = {0};
	if(operand->operandClass == REG && operand->laneUsed)
		snprintf(index, sizeof(index), "[%u]", operand->lane);

	if(snprintf(outBuffer, outBufferSize, "%s%s", reg_buf, index) >= outBufferSize)
		return FAILED_TO_DISASSEMBLE_REGISTER;

	return 0;
}

uint32_t get_multireg_operand(const InstructionOperand *operand, char *result, uint32_t result_sz)
{
	char lane_str[32] = {0};
	char reg_str[4][32];
	uint32_t elem_n;
	int rc;
	memset(&reg_str, 0, sizeof(reg_str));

	for (elem_n = 0; elem_n < 4 && operand->reg[elem_n] != REG_NONE; elem_n++)
		if (get_register(operand, elem_n, reg_str[elem_n], 32) != 0)
			return FAILED_TO_DISASSEMBLE_OPERAND;

	if(operand->laneUsed)
		snprintf(lane_str, sizeof(lane_str), "[%d]", operand->lane);

	switch (elem_n)
	{
		case 1:
			rc = snprintf(result, result_sz, "{%s}%s",
				reg_str[0], lane_str);
			break;
		case 2:
			rc = snprintf(result, result_sz, "{%s, %s}%s",
				reg_str[0], reg_str[1], lane_str);
			break;
		case 3:
			rc = snprintf(result, result_sz, "{%s, %s, %s}%s",
				reg_str[0], reg_str[1], reg_str[2], lane_str);
			break;
		case 4:
			rc = snprintf(result, result_sz, "{%s, %s, %s, %s}%s",
				reg_str[0], reg_str[1], reg_str[2], reg_str[3], lane_str);
			break;
		default:
			return FAILED_TO_DISASSEMBLE_OPERAND;
	}

	return rc < 0 ? FAILED_TO_DISASSEMBLE_OPERAND : DISASM_SUCCESS;
}

uint32_t get_shifted_immediate(const InstructionOperand *instructionOperand, char *outBuffer, uint32_t outBufferSize, uint32_t type)
{
	char shiftBuff[48] = {0};
	char immBuff[32] = {0};
	const char *sign = "";
	if (instructionOperand == NULL)
		return FAILED_TO_DISASSEMBLE_OPERAND;

	uint64_t imm = instructionOperand->immediate;
	if (instructionOperand->signedImm == 1 && ((int64_t)imm) < 0)
	{
		sign = "-";
		imm = -(int64_t)imm;
	}
	if (instructionOperand->shiftType != ShiftType_NONE)
	{
		if (instructionOperand->shiftValueUsed != 0)
		{
			if (snprintf(immBuff, sizeof(immBuff), " #%#x", instructionOperand->shiftValue) >= sizeof(immBuff))
			{
				return FAILED_TO_DISASSEMBLE_REGISTER;
			}
		}
		const char *shiftStr = get_shift(instructionOperand->shiftType);
		if (shiftStr == NULL)
			return FAILED_TO_DISASSEMBLE_OPERAND;
		snprintf(
				shiftBuff,
				sizeof(shiftBuff),
				", %s%s",
				shiftStr,
				immBuff);
	}
	if (type == FIMM32)
	{
		float f = *(const float*)&instructionOperand->immediate;
		if (snprintf(outBuffer, outBufferSize, "#%.08f%s", f, shiftBuff) >= outBufferSize)
			return FAILED_TO_DISASSEMBLE_OPERAND;
	}
	else if (type == IMM32)
	{
		if (snprintf(outBuffer, outBufferSize, "#%s%#x%s", sign, (uint32_t)imm, shiftBuff) >= outBufferSize)
			return FAILED_TO_DISASSEMBLE_OPERAND;
	}
	else if (type == LABEL)
	{
		if (snprintf(outBuffer, outBufferSize, "0x%" PRIx64, (uint64_t)imm) >= outBufferSize)
			return FAILED_TO_DISASSEMBLE_OPERAND;
	}
	else if (type == STR_IMM)
	{
		if (snprintf(outBuffer, outBufferSize, "%s #0x%" PRIx64, instructionOperand->name, (uint64_t)imm) >= outBufferSize)
			return FAILED_TO_DISASSEMBLE_OPERAND;
	}
	else
	{
		if (snprintf(outBuffer, outBufferSize, "#%s%#" PRIx64 "%s",
					sign,
					imm,
					shiftBuff) >= outBufferSize)
			return FAILED_TO_DISASSEMBLE_OPERAND;
	}
	return DISASM_SUCCESS;
}

uint32_t get_sme_tile(const InstructionOperand *operand, char *outBuffer, uint32_t outBufferSize)
{
	char base_offset[32] = {'\0'};
	if(operand->reg[0] != REG_NONE) {
		if(operand->arrSpec == ARRSPEC_FULL)
			snprintf(base_offset, sizeof(base_offset), "[%s]", get_register_name(operand->reg[0]));
		else
			snprintf(base_offset, sizeof(base_offset), "[%s, #%" PRIu64 "]", get_register_name(operand->reg[0]), operand->immediate);
	}

	char *slice = "";
	if(operand->slice == SLICE_HORIZONTAL)
		slice = "H";
	else if(operand->slice == SLICE_VERTICAL)
		slice = "V";

	if(snprintf(outBuffer, outBufferSize, "Z%d%s%s%s",
	  operand->tile,
	  slice,
	  get_arrspec_str_truncated(operand->arrSpec),
	  base_offset
	 ) >= outBufferSize)
		return FAILED_TO_DISASSEMBLE_OPERAND;

	return DISASM_SUCCESS;
}

uint32_t get_indexed_element(const InstructionOperand *operand, char *outBuffer, uint32_t outBufferSize)
{
	// make the "{, #<imm>}"
	char optional_comma_and[32];
	if(operand->immediate)
		if(snprintf(optional_comma_and, 32, ", #%" PRIu64 "", operand->immediate) >= 32)
			return FAILED_TO_DISASSEMBLE_OPERAND;

	// <Pn>.<T>[<Wm>{, #<imm>}]
	if(snprintf(outBuffer, outBufferSize, "%s%s[%s%s]",
		get_register_name(operand->reg[0]),
		get_arrspec_str_truncated(operand->arrSpec),
		get_register_name(operand->reg[1]),
		optional_comma_and
	  ) >= outBufferSize)
	  	return FAILED_TO_DISASSEMBLE_OPERAND;

	return DISASM_SUCCESS;
}

uint32_t get_accum_array(const InstructionOperand *operand, char *outBuffer, uint32_t outBufferSize)
{
	if(snprintf(outBuffer, outBufferSize, "ZA[%s, #%" PRIu64 "]",
	  get_register_name(operand->reg[0]), operand->immediate
	  ) >= outBufferSize)
		return FAILED_TO_DISASSEMBLE_OPERAND;

	return DISASM_SUCCESS;
}

//-----------------------------------------------------------------------------
// disassemble (decoded Instruction -> string)
//-----------------------------------------------------------------------------

int aarch64_disassemble(Instruction *instruction, char *buf, size_t buf_sz)
{
	char operandStrings[MAX_OPERANDS][130];
	char tmpOperandString[128];
	const char *operand = tmpOperandString;
	if (instruction == NULL || buf_sz == 0 || buf == NULL)
		return INVALID_ARGUMENTS;

	memset(operandStrings, 0, sizeof(operandStrings));
	const char *operation = get_operation(instruction);
	if (operation == NULL)
		return FAILED_TO_DISASSEMBLE_OPERATION;

	for(int i=0; i<MAX_OPERANDS; i++)
		memset(&(operandStrings[i][0]), 0, 128);

	for(int i=0; i<MAX_OPERANDS && instruction->operands[i].operandClass != NONE; i++)
	{
		switch (instruction->operands[i].operandClass)
		{
			case CONDITION:
				if (snprintf(tmpOperandString, sizeof(tmpOperandString), "%s",
							get_condition((Condition)instruction->operands[i].cond)) >= sizeof(tmpOperandString))
					return FAILED_TO_DISASSEMBLE_OPERAND;
				operand = tmpOperandString;
				break;
			case FIMM32:
			case IMM32:
			case IMM64:
			case LABEL:
			case STR_IMM:
				if (get_shifted_immediate(
							&instruction->operands[i],
							tmpOperandString,
							sizeof(tmpOperandString),
							instruction->operands[i].operandClass) != DISASM_SUCCESS)
					return FAILED_TO_DISASSEMBLE_OPERAND;
				operand = tmpOperandString;
				break;
			case REG:
				if (get_register(
						&instruction->operands[i],
						0,
						tmpOperandString,
						sizeof(tmpOperandString)) != DISASM_SUCCESS)
					return FAILED_TO_DISASSEMBLE_OPERAND;
				operand = tmpOperandString;
				break;
			case SYS_REG:
				operand = get_system_register_name(instruction->operands[i].sysreg);
				if (operand == NULL)
				{
					return FAILED_TO_DISASSEMBLE_OPERAND;
				}
				break;
			case MULTI_REG:
				if (get_multireg_operand(
							&instruction->operands[i],
							tmpOperandString,
							sizeof(tmpOperandString)) != DISASM_SUCCESS)
				{
					return FAILED_TO_DISASSEMBLE_OPERAND;
				}
				operand = tmpOperandString;
				break;
			case IMPLEMENTATION_SPECIFIC:
				if (get_implementation_specific(
						&instruction->operands[i],
						tmpOperandString,
						sizeof(tmpOperandString)) != DISASM_SUCCESS)
				{
					return FAILED_TO_DISASSEMBLE_OPERAND;
				}
				operand = tmpOperandString;
				break;
			case MEM_REG:
			case MEM_OFFSET:
			case MEM_EXTENDED:
			case MEM_PRE_IDX:
			case MEM_POST_IDX:
				if (get_memory_operand(&instruction->operands[i],
							tmpOperandString,
							sizeof(tmpOperandString)) != DISASM_SUCCESS)
					return FAILED_TO_DISASSEMBLE_OPERAND;
				operand = tmpOperandString;
				break;
			case SME_TILE:
				if(get_sme_tile(&instruction->operands[i],
					tmpOperandString,
					sizeof(tmpOperandString)) != DISASM_SUCCESS)
					return FAILED_TO_DISASSEMBLE_OPERAND;
				operand = tmpOperandString;
				break;
			case INDEXED_ELEMENT:
				if(get_indexed_element(&instruction->operands[i],
					tmpOperandString,
					sizeof(tmpOperandString)) != DISASM_SUCCESS)
					return FAILED_TO_DISASSEMBLE_OPERAND;
				operand = tmpOperandString;
				break;
			case ACCUM_ARRAY:
				if(get_accum_array(&instruction->operands[i],
					tmpOperandString,
					sizeof(tmpOperandString)) != DISASM_SUCCESS)
					return FAILED_TO_DISASSEMBLE_OPERAND;
				operand = tmpOperandString;
				break;
			case NAME:
				operand = instruction->operands[i].name;
				break;
			case NONE:
				break;
		}
		snprintf(operandStrings[i], sizeof(operandStrings[i]), i==0?"\t%s":", %s", operand);
	}
	memset(buf, 0, buf_sz);
	if (snprintf(buf, buf_sz, "%s%s%s%s%s%s",
				get_operation(instruction),
				operandStrings[0],
				operandStrings[1],
				operandStrings[2],
				operandStrings[3],
				operandStrings[4]) >= buf_sz)
		return OUTPUT_BUFFER_TOO_SMALL;
	return DISASM_SUCCESS;
}

void print_instruction(Instruction *instr)
{
	//printf("print_instruction (TODO)\n");
}
