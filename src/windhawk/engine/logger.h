#pragma once

#include "logger_base.h"

class Logger : public LoggerBase {
   public:
    // Use to temporarily change the verbosity of the current thread.
    class ScopedThreadVerbosity {
       public:
        ScopedThreadVerbosity(Verbosity verbosity);
        ~ScopedThreadVerbosity();

        ScopedThreadVerbosity(const ScopedThreadVerbosity&) = delete;
        ScopedThreadVerbosity(ScopedThreadVerbosity&&) = delete;
        ScopedThreadVerbosity& operator=(const ScopedThreadVerbosity&) = delete;
        ScopedThreadVerbosity& operator=(ScopedThreadVerbosity&&) = delete;

       private:
        bool m_inUse = false;
    };

    Logger(Verbosity initialVerbosity);

    static Logger& GetInstance();

    bool ShouldLog(Verbosity verbosity);

   private:
    static std::optional<Verbosity>& GetThreadVerbosity();
    bool SetThreadVerbosity(Verbosity verbosity);
    void ResetThreadVerbosity();

    const std::atomic<Verbosity> m_initialVerbosity;
    std::mutex m_threadVerbosityMutex;
    int m_threadVerbosityCount = 0;
};

#define LOG_WITH_VERBOSITY(verbosity, message, ...)                          \
    do {                                                                     \
        auto& inst = Logger::GetInstance();                                  \
        if (inst.GetVerbosity() >= verbosity && inst.ShouldLog(verbosity)) { \
            inst.LogLine(L"[WH] [%S]: " message L"\n", __FUNCTION__,         \
                         __VA_ARGS__);                                       \
        }                                                                    \
    } while (0)

#define LOG(message, ...) \
    LOG_WITH_VERBOSITY(Logger::Verbosity::kOn, message, __VA_ARGS__)
#define VERBOSE(message, ...) \
    LOG_WITH_VERBOSITY(Logger::Verbosity::kVerbose, message, __VA_ARGS__)
