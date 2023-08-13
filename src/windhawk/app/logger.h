#pragma once

#include "logger_base.h"

class Logger : public LoggerBase {
   public:
    Logger(Verbosity initialVerbosity);

    static Logger& GetInstance();
};

#define LOG_WITH_VERBOSITY(verbosity, message, ...)                  \
    do {                                                             \
        auto& inst = Logger::GetInstance();                          \
        if (inst.GetVerbosity() >= verbosity) {                      \
            inst.LogLine(L"[WH] [%S]: " message L"\n", __FUNCTION__, \
                         __VA_ARGS__);                               \
        }                                                            \
    } while (0)

#define LOG(message, ...) \
    LOG_WITH_VERBOSITY(Logger::Verbosity::kOn, message, __VA_ARGS__)
#define VERBOSE(message, ...) \
    LOG_WITH_VERBOSITY(Logger::Verbosity::kVerbose, message, __VA_ARGS__)
