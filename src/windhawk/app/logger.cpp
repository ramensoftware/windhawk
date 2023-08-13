#include "stdafx.h"

#include "logger.h"

#include "storage_manager.h"

namespace {

Logger::Verbosity GetVerbosityFromConfig() {
    try {
        auto settings =
            StorageManager::GetInstance().GetAppConfig(L"Settings", false);
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

Logger::Logger(Verbosity initialVerbosity) : LoggerBase(initialVerbosity) {}

// static
Logger& Logger::GetInstance() {
    static Logger s(GetVerbosityFromConfig());
    return s;
}
