#pragma once

namespace Functions {

bool wcsmatch(PCWSTR pat, size_t plen, PCWSTR str, size_t slen);
bool DoesPathMatchPattern(std::wstring_view path,
                          std::wstring_view pattern,
                          bool explicitOnly = false);

}  // namespace Functions
