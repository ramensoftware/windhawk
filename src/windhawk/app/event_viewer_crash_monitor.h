#pragma once

class EventViewerCrashMonitor {
   public:
    EventViewerCrashMonitor(std::wstring_view targetAppPath);

    HANDLE GetEventHandle() const;
    int GetAmountOfNewEvents();

   private:
    bool DoesEventMatch(EVT_HANDLE eventHandle);

    std::wstring m_targetAppPath;
    wil::unique_event m_event;
    wil::unique_evt_handle m_subscription;
    DWORD m_lastProcessId = 0;
    DWORD64 m_lastProcessCreationTime = 0;
};
