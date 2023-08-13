#include "stdafx.h"

#include "logger_base.h"

LoggerBase::LoggerBase(Verbosity initialVerbosity)
    : m_verbosity(initialVerbosity) {}

void LoggerBase::SetVerbosity(Verbosity verbosity) {
    m_verbosity = verbosity;
}

LoggerBase::Verbosity LoggerBase::GetVerbosity() {
    return m_verbosity;
}

void LoggerBase::VLogLine(PCWSTR format, va_list args) {
    WCHAR buffer[1025];
    int len = _vsnwprintf_s(buffer, _TRUNCATE, format, args);
    if (len == -1) {
        // Truncation occurred.
        len = _countof(buffer) - 1;
    }

    while (--len >= 0 && buffer[len] == L'\n') {
        // Skip all newlines at the end.
    }

    // Leave only a single trailing newline.
    if (buffer[len + 1] == L'\n' && buffer[len + 2] == L'\n') {
        buffer[len + 2] = L'\0';
    }

    OutputDebugString(buffer);
}

void LoggerBase::LogLine(PCWSTR format, ...) {
    va_list args;
    va_start(args, format);
    VLogLine(format, args);
    va_end(args);
}
