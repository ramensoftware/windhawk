#pragma once

class LoggerBase {
   public:
    enum class Verbosity {
        kOff,
        kOn,
        kVerbose,
    };

    static constexpr auto kDefaultVerbosity = Verbosity::kOn;

    LoggerBase(Verbosity initialVerbosity);

    void SetVerbosity(Verbosity verbosity);
    Verbosity GetVerbosity();
    void VLogLine(PCWSTR format, va_list args);
    void LogLine(PCWSTR format, ...);

   private:
    std::atomic<Verbosity> m_verbosity = kDefaultVerbosity;
};
