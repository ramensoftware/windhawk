#pragma once

#include <stdint.h>

#ifdef __cplusplus
extern "C"
{
#endif

	int aarch64_decompose_and_disassemble(uint64_t address, uint32_t insword, char* result, size_t result_buf_sz);

#ifdef __cplusplus
}
#endif
