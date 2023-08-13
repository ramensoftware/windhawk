#include "stdafx.h"

#include "logger.h"
#include "storage_manager.h"
#include "var_init_once.h"

namespace {

Logger::Verbosity GetVerbosityFromConfig() {
    try {
        auto settings = StorageManager::GetInstance().GetAppConfig(L"Settings");
        int verbosity = settings->GetInt(L"LoggingVerbosity").value_or(0);

        switch (verbosity) {
            case static_cast<int>(Logger::Verbosity::kOff):
                return Logger::Verbosity::kOff;

            case static_cast<int>(Logger::Verbosity::kOn):
                return Logger::Verbosity::kOn;

            case static_cast<int>(Logger::Verbosity::kVerbose):
                return Logger::Verbosity::kVerbose;
        }
    } catch (const std::exception&) {
        // Ignore and use default settings. We can't log it, anyway.
    }

    return Logger::kDefaultVerbosity;
}

}  // namespace

Logger::ScopedThreadVerbosity::ScopedThreadVerbosity(Verbosity verbosity) {
    m_inUse = GetInstance().SetThreadVerbosity(verbosity);
}

Logger::ScopedThreadVerbosity::~ScopedThreadVerbosity() {
    if (m_inUse) {
        GetInstance().ResetThreadVerbosity();
    }
}

Logger::Logger(Verbosity initialVerbosity)
    : m_initialVerbosity(initialVerbosity), LoggerBase(initialVerbosity) {}

// static
Logger& Logger::GetInstance() {
    STATIC_INIT_ONCE(Logger, s, GetVerbosityFromConfig());
    return *s;
}

bool Logger::ShouldLog(Verbosity verbosity) {
    auto& threadVerbosity = GetThreadVerbosity();
    return threadVerbosity ? *threadVerbosity >= verbosity
                           : m_initialVerbosity >= verbosity;
}

// static
std::optional<Logger::Verbosity>& Logger::GetThreadVerbosity() {
    STATIC_INIT_ONCE(ThreadLocal<std::optional<Verbosity>>, s);
    return *s;
}

bool Logger::SetThreadVerbosity(Verbosity verbosity) {
    auto& threadVerbosity = GetThreadVerbosity();
    if (threadVerbosity) {
        // Only one ScopedThreadVerbosity is supported at a time.
        return false;
    }

    threadVerbosity = verbosity;

    std::lock_guard guard(m_threadVerbosityMutex);

    m_threadVerbosityCount++;

    if (GetVerbosity() < verbosity) {
        SetVerbosity(verbosity);
    }

    return true;
}

void Logger::ResetThreadVerbosity() {
    auto& threadVerbosity = GetThreadVerbosity();
    threadVerbosity.reset();

    std::lock_guard guard(m_threadVerbosityMutex);

    if (--m_threadVerbosityCount == 0) {
        SetVerbosity(m_initialVerbosity);
    }
}
