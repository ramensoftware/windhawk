#include "stdafx.h"

#include "event_viewer_crash_monitor.h"

#include "logger.h"

// Based on:
// https://learn.microsoft.com/en-us/windows/win32/wes/subscribing-to-events#push-subscriptions
// https://learn.microsoft.com/en-us/windows/win32/wes/rendering-events

EventViewerCrashMonitor::EventViewerCrashMonitor(
    std::wstring_view targetAppPath)
    : m_targetAppPath(targetAppPath) {
    // Get a handle to an event object that the subscription will signal when
    // events become available that match your query criteria.
    m_event.reset(CreateEvent(nullptr, TRUE, TRUE, nullptr));
    THROW_LAST_ERROR_IF_NULL(m_event);

    PCWSTR pwsPath = L"Application";
    PCWSTR pwsQuery = L"Event/System[Level=2] and Event/System[EventID=1000]";

    // Subscribe to events.
    m_subscription.reset(EvtSubscribe(nullptr, m_event.get(), pwsPath, pwsQuery,
                                      nullptr, nullptr, nullptr,
                                      EvtSubscribeToFutureEvents));
    THROW_LAST_ERROR_IF_NULL(m_subscription);

    LCMapStringEx(
        LOCALE_NAME_USER_DEFAULT, LCMAP_UPPERCASE, &m_targetAppPath[0],
        wil::safe_cast<int>(m_targetAppPath.length()), &m_targetAppPath[0],
        wil::safe_cast<int>(m_targetAppPath.length()), nullptr, nullptr, 0);
}

HANDLE EventViewerCrashMonitor::GetEventHandle() const {
    return m_event.get();
}

int EventViewerCrashMonitor::GetAmountOfNewEvents() {
    int count = 0;

    while (true) {
        // Get a block of events from the result set.
        wil::unique_evt_handle eventHandle;
        DWORD dwReturned;
        if (!EvtNext(m_subscription.get(), 1, &eventHandle, INFINITE, 0,
                     &dwReturned)) {
            THROW_LAST_ERROR_IF(GetLastError() != ERROR_NO_MORE_ITEMS);
            break;
        }

        try {
            if (DoesEventMatch(eventHandle.get())) {
                count++;
            }
        } catch (const std::exception& e) {
            LOG(L"%S", e.what());
        }
    }

    ResetEvent(m_event.get());

    return count;
}

bool EventViewerCrashMonitor::DoesEventMatch(EVT_HANDLE eventHandle) {
    // Identify the components of the event that you want to render. In this
    // case, render the user section of the event.
    wil::unique_evt_handle context(
        EvtCreateRenderContext(0, nullptr, EvtRenderContextUser));
    THROW_LAST_ERROR_IF_NULL(context);

    // When you render the user data or system section of the event, you must
    // specify the EvtRenderEventValues flag. The function returns an array of
    // variant values for each element in the user data or system section of the
    // event. For user data or event data, the values are returned in the same
    // order as the elements are defined in the event. For system data, the
    // values are returned in the order defined in the EVT_SYSTEM_PROPERTY_ID
    // enumeration.
    DWORD dwBufferSize = 0;
    DWORD dwBufferUsed = 0;
    DWORD dwPropertyCount = 0;

    if (EvtRender(context.get(), eventHandle, EvtRenderEventValues,
                  dwBufferSize, nullptr, &dwBufferUsed, &dwPropertyCount)) {
        throw std::logic_error("Unexpected result from EvtRender");
    }

    THROW_LAST_ERROR_IF(GetLastError() != ERROR_INSUFFICIENT_BUFFER);

    dwBufferSize = dwBufferUsed;
    auto renderedValuesBuffer = std::make_unique<BYTE[]>(dwBufferSize);
    THROW_IF_WIN32_BOOL_FALSE(EvtRender(
        context.get(), eventHandle, EvtRenderEventValues, dwBufferSize,
        renderedValuesBuffer.get(), &dwBufferUsed, &dwPropertyCount));

    if (dwPropertyCount < 11) {
        LOG(L"Not enough property values (%u)", dwPropertyCount);
        return false;
    }

    auto renderedValues =
        reinterpret_cast<const EVT_VARIANT*>(renderedValuesBuffer.get());

    if (renderedValues[10].Type != EvtVarTypeString) {
        LOG(L"Unexpected property value type (%u)", renderedValues[10].Type);
        return false;
    }

    std::wstring appPath{renderedValues[10].StringVal};
    LCMapStringEx(LOCALE_NAME_USER_DEFAULT, LCMAP_UPPERCASE, &appPath[0],
                  wil::safe_cast<int>(appPath.length()), &appPath[0],
                  wil::safe_cast<int>(appPath.length()), nullptr, nullptr, 0);

    if (appPath != m_targetAppPath) {
        return false;
    }

    DWORD processId = renderedValues[8].Type == EvtVarTypeHexInt32
                          ? renderedValues[8].Int32Val
                          : 0;
    DWORD64 processCreationTime = renderedValues[9].Type == EvtVarTypeHexInt64
                                      ? renderedValues[9].Int64Val
                                      : 0;

    if (processId && processCreationTime && processId == m_lastProcessId &&
        processCreationTime == m_lastProcessCreationTime) {
        return false;
    }

    m_lastProcessId = processId;
    m_lastProcessCreationTime = processCreationTime;

    return true;
}
