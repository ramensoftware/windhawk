#include "stdafx.h"

#include "task_manager_dlg.h"

#include "functions.h"
#include "logger.h"
#include "storage_manager.h"

namespace {

// Wait for a bit before refreshing the list, in case more changes will follow.
constexpr auto kRefreshListOnDataChangeDelay = 200;

constexpr auto kUpdateProcessesStatusInterval = 1000;

struct ListItemData {
    std::wstring filePath;
    std::wstring processName;
    DWORD processId = 0;
    ULONGLONG creationTime = 0;
    bool isFrozen = false;
    wil::unique_process_handle executionRequiredRequestProcess;
    wil::unique_handle executionRequiredRequest;
};

bool CanShowDialog() {
    QUERY_USER_NOTIFICATION_STATE pquns;
    if (FAILED(SHQueryUserNotificationState(&pquns))) {
        return true;
    }

    // Prevent the dialog from being shown when in fullscreen mode, which can
    // cause troubles such as interrupting a fullscreen game.
    switch (pquns) {
        case QUNS_NOT_PRESENT:
        case QUNS_BUSY:
        case QUNS_RUNNING_D3D_FULL_SCREEN:
            return false;
    }

    return true;
}

std::wstring GetMetadataContent(PCWSTR filePath, FILETIME* pCreationTime) {
    wil::unique_hfile file(
        CreateFile(filePath, GENERIC_READ,
                   FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                   nullptr, OPEN_EXISTING, 0, nullptr));
    THROW_LAST_ERROR_IF(!file);

    LARGE_INTEGER fileSizeLarge;
    THROW_IF_WIN32_BOOL_FALSE(GetFileSizeEx(file.get(), &fileSizeLarge));

    DWORD fileSize = 0;
    if (fileSizeLarge.QuadPart <= DWORD_MAX ||
        (fileSizeLarge.QuadPart % sizeof(WCHAR)) == 0) {
        fileSize = static_cast<DWORD>(fileSizeLarge.QuadPart);
    }

    std::wstring fileContents(fileSize / sizeof(WCHAR), L'\0');

    DWORD numberOfBytesRead;
    THROW_IF_WIN32_BOOL_FALSE(ReadFile(file.get(), fileContents.data(),
                                       fileSize, &numberOfBytesRead, nullptr));

    if (pCreationTime) {
        THROW_IF_WIN32_BOOL_FALSE(
            GetFileTime(file.get(), pCreationTime, nullptr, nullptr));
    }

    return fileContents;
}

std::wstring LocalizeStatus(PCWSTR status) {
    static const std::unordered_map<std::wstring_view, UINT> translation = {
        {L"Pending...", IDS_TASKDLG_STATUS_PENDING},
        {L"Loading...", IDS_TASKDLG_STATUS_LOADING},
        {L"Loaded", IDS_TASKDLG_STATUS_LOADED},
        {L"Unloaded", IDS_TASKDLG_STATUS_UNLOADED},
        {L"Initializing...", IDS_TASKDLG_TASK_INITIALIZING},
        {L"Uninitializing...", IDS_TASKDLG_TASK_UNINITIALIZING},
    };
    auto it = translation.find(status);
    if (it != translation.end()) {
        return Functions::LoadStrFromRsrc(it->second);
    }

    std::wstring_view statusView{status};

    std::wstring_view prefixLoading = L"Loading symbols...";
    if (statusView.starts_with(prefixLoading)) {
        return Functions::LoadStrFromRsrc(IDS_TASKDLG_TASK_LOADING_SYMBOLS) +
               std::wstring(statusView.substr(prefixLoading.length()));
    }

    std::wstring_view prefixWaiting = L"Waiting for symbols...";
    if (statusView.starts_with(prefixWaiting)) {
        return Functions::LoadStrFromRsrc(
                   IDS_TASKDLG_TASK_WAITING_FOR_SYMBOLS) +
               std::wstring(statusView.substr(prefixWaiting.length()));
    }

    return status;
}

bool IsProcessFrozen(DWORD processId) {
    wil::unique_process_handle process(
        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, processId));
    if (!process) {
        return false;
    }

    return Functions::IsProcessFrozen(process.get());
}

}  // namespace

// static
bool CTaskManagerDlg::IsDataSourceEmpty(DataSource dataSource) {
    PCWSTR metadataCategory = nullptr;
    switch (dataSource) {
        case DataSource::kModStatus:
            metadataCategory = L"mod-status";
            break;

        case DataSource::kModTask:
            metadataCategory = L"mod-task";
            break;
    }

    auto metadataPath =
        StorageManager::GetInstance().GetModMetadataPath(metadataCategory);

    if (!std::filesystem::exists(metadataPath)) {
        return true;
    }

    for (const auto& p : std::filesystem::directory_iterator(metadataPath)) {
        if (!p.is_regular_file()) {
            continue;
        }

        return false;
    }

    return true;
}

CTaskManagerDlg::CTaskManagerDlg(DialogOptions dialogOptions)
    : m_dialogOptions(std::move(dialogOptions)) {}

void CTaskManagerDlg::LoadLanguageStrings() {
    UINT titleId = 0;
    switch (m_dialogOptions.dataSource) {
        case DataSource::kModStatus:
            titleId = IDS_TASKDLG_TITLE_LOADED_MODS;
            break;

        case DataSource::kModTask:
            titleId = IDS_TASKDLG_TITLE_TASKS_IN_PROGRESS;
            break;
    }

    WCHAR title[1024] = L"Windhawk";
    if (titleId) {
        _snwprintf_s(title, _TRUNCATE, L"%s - Windhawk",
                     Functions::LoadStrFromRsrc(titleId));
    }
    SetWindowText(title);

    SetDlgItemText(IDOK,
                   Functions::LoadStrFromRsrc(IDS_TASKDLG_BUTTON_OPEN_APP));

    UINT columnStringIds[] = {
        IDS_TASKDLG_COLUMN_MOD,
        IDS_TASKDLG_COLUMN_PROCESS,
        IDS_TASKDLG_COLUMN_PID,
        IDS_TASKDLG_COLUMN_STATUS,
    };

    for (int i = 0; i < ARRAYSIZE(columnStringIds); i++) {
        LVCOLUMN column = {LVCF_TEXT};
        column.pszText = (PWSTR)Functions::LoadStrFromRsrc(columnStringIds[i]);
        m_taskListSort.SetColumn(i, &column);
    }

    bool languageRightToLeft =
        Functions::IsRightToLeftLanguage(GetThreadUILanguage());
    Functions::ApplyDialogLayoutRtl(*this, languageRightToLeft);
}

void CTaskManagerDlg::DataChanged() {
    if (m_refreshListOnDataChangePending) {
        return;
    }

    SetTimer(Timer::kRefreshList, kRefreshListOnDataChangeDelay);
    m_refreshListOnDataChangePending = true;
}

BOOL CTaskManagerDlg::OnInitDialog(CWindow wndFocus, LPARAM lInitParam) {
    ReloadMainIcon();

    DlgResize_Init();
    m_ptMinTrackSize.x /= 2;
    m_ptMinTrackSize.y /= 2;

    if (m_dialogOptions.autonomousMode) {
        ModifyStyle(WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_SYSMENU, 0);
        ModifyStyleEx(0, WS_EX_TOOLWINDOW);

        // Make the window topmost, slightly wider and less high.
        CRect rect;
        GetWindowRect(&rect);
        rect.right += rect.Width() / 4;
        rect.bottom -= rect.Height() / 3;
        SetWindowPos(HWND_TOPMOST, rect, SWP_NOMOVE | SWP_NOACTIVATE);

        PlaceWindowAtTrayArea();
    } else {
        ModifyStyleEx(0, WS_EX_APPWINDOW);
        CenterWindow();
    }

    InitTaskList();

    LoadLanguageStrings();

    SetTimer(Timer::kUpdateProcessesStatus, kUpdateProcessesStatusInterval);

    if (!m_dialogOptions.autonomousMode) {
        try {
            LoadTaskList();
        } catch (const std::exception& e) {
            ::MessageBoxA(m_hWnd, e.what(), "Failed to initialize data",
                          MB_ICONERROR);
            DestroyWindow();
            return FALSE;
        }

        return TRUE;
    } else {
        SetTimer(Timer::kRefreshList, kRefreshListOnDataChangeDelay);
        m_refreshListOnDataChangePending = true;

        SetTimer(Timer::kShowDlg,
                 std::max(m_dialogOptions.autonomousModeShowDelay,
                          kAutonomousModeShowDelayMin));
        m_showDlgPending = true;

        return FALSE;
    }
}

void CTaskManagerDlg::OnDestroy() {
    KillTimer(Timer::kUpdateProcessesStatus);

    if (m_refreshListOnDataChangePending) {
        KillTimer(Timer::kRefreshList);
    }

    if (m_showDlgPending) {
        KillTimer(Timer::kShowDlg);
    }

    int count = m_taskListSort.GetItemCount();
    for (int i = 0; i < count; i++) {
        delete reinterpret_cast<ListItemData*>(m_taskListSort.GetItemData(i));
    }

    // From GDI handle checks, not all icons are freed automatically.
    ::DestroyIcon(SetIcon(nullptr, TRUE));
    ::DestroyIcon(SetIcon(nullptr, FALSE));
}

void CTaskManagerDlg::OnTimer(UINT_PTR nIDEvent) {
    switch ((Timer)nIDEvent) {
        case Timer::kUpdateProcessesStatus:
            UpdateTaskListProcessesStatus();
            break;

        case Timer::kRefreshList:
            KillTimer(Timer::kRefreshList);
            m_refreshListOnDataChangePending = false;
            RefreshTaskList();
            break;

        case Timer::kShowDlg:
            KillTimer(Timer::kShowDlg);

            if (CanShowDialog()) {
                m_showDlgPending = false;
                ShowWindow(SW_SHOWNA);
            } else {
                SetTimer(Timer::kShowDlg, kUpdateProcessesStatusInterval);
            }
            break;
    }
}

void CTaskManagerDlg::OnDpiChanged(UINT nDpiX, UINT nDpiY, PRECT pRect) {
    ReloadMainIcon();
}

void CTaskManagerDlg::OnOK(UINT uNotifyCode, int nID, CWindow wndCtl) {
    if (m_dialogOptions.runButtonCallback) {
        m_dialogOptions.runButtonCallback(m_hWnd);
    }
}

void CTaskManagerDlg::OnCancel(UINT uNotifyCode, int nID, CWindow wndCtl) {
    if (m_dialogOptions.autonomousMode) {
        return;
    }

    DestroyWindow();
}

LRESULT CTaskManagerDlg::OnListRightClick(LPNMHDR pnmh) {
    // LPNMITEMACTIVATE pnmItemActivate = (LPNMITEMACTIVATE)pnmh;

    // if (m_taskListSort.GetItemCount() == 0) {
    //     return 1;
    // }

    return 1;
}

void CTaskManagerDlg::OnFinalMessage(HWND hWnd) {
    if (m_dialogOptions.finalMessageCallback) {
        m_dialogOptions.finalMessageCallback(m_hWnd);
    }
}

UINT_PTR CTaskManagerDlg::SetTimer(Timer nIDEvent,
                                   UINT nElapse,
                                   TIMERPROC lpfnTimer) {
    return CDialogImpl::SetTimer(static_cast<UINT_PTR>(nIDEvent), nElapse,
                                 lpfnTimer);
}

BOOL CTaskManagerDlg::KillTimer(Timer nIDEvent) {
    return CDialogImpl::KillTimer(static_cast<UINT_PTR>(nIDEvent));
}

void CTaskManagerDlg::ReloadMainIcon() {
    UINT dpi = Functions::GetDpiForWindowWithFallback(m_hWnd);

    CIconHandle mainIcon;
    mainIcon.LoadIconWithScaleDown(
        IDR_MAINFRAME,
        Functions::GetSystemMetricsForDpiWithFallback(SM_CXICON, dpi),
        Functions::GetSystemMetricsForDpiWithFallback(SM_CYICON, dpi));
    CIcon prevMainIcon = SetIcon(mainIcon, TRUE);

    CIconHandle mainIconSmall;
    mainIconSmall.LoadIconWithScaleDown(
        IDR_MAINFRAME,
        Functions::GetSystemMetricsForDpiWithFallback(SM_CXSMICON, dpi),
        Functions::GetSystemMetricsForDpiWithFallback(SM_CYSMICON, dpi));
    CIcon prevMainIconSmall = SetIcon(mainIconSmall, FALSE);
}

void CTaskManagerDlg::PlaceWindowAtTrayArea() {
    CRect windowRect;
    GetWindowRect(&windowRect);

    CRect workAreaRect;
    ::SystemParametersInfo(SPI_GETWORKAREA, 0, &workAreaRect, 0);

    int margin = 8;

    windowRect.MoveToXY(workAreaRect.right - windowRect.Width() - margin,
                        workAreaRect.bottom - windowRect.Height() - margin);

    SetWindowPos(nullptr, windowRect,
                 SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
}

void CTaskManagerDlg::InitTaskList() {
    CListViewCtrl list(GetDlgItem(IDC_TASK_LIST));

    m_taskListSort.SubclassWindow(list);

    list.SetExtendedListViewStyle(LVS_EX_HEADERDRAGDROP | LVS_EX_FULLROWSELECT |
                                  LVS_EX_LABELTIP | LVS_EX_DOUBLEBUFFER);
    ::SetWindowTheme(list, L"Explorer", nullptr);

    UINT windowDpi = Functions::GetDpiForWindowWithFallback(m_hWnd);

    // Sort PIDs as decimals, signed 128-bit (16-byte) values representing
    // 96-bit (12-byte) integer numbers:
    // https://learn.microsoft.com/en-us/dotnet/visual-basic/language-reference/data-types/decimal-data-type
    // A bit of an overkill, but LVCOLSORT_LONG is signed 32-bit while PIDs are
    // unsigned 32-bit, so it doesn't fit. And a custom type isn't worth the
    // effort.
    struct {
        PCWSTR name;
        int width;
        WORD sort;
    } columns[] = {
        {L"Mod", 160, LVCOLSORT_TEXT},
        {L"Process", 80, LVCOLSORT_TEXT},
        {L"PID", 60, LVCOLSORT_DECIMAL},
        {L"Status", LVSCW_AUTOSIZE_USEHEADER, LVCOLSORT_TEXT},
    };

    for (int i = 0; i < ARRAYSIZE(columns); i++) {
        list.InsertColumn(i, columns[i].name);
        int width = columns[i].width;
        if (width > 0) {
            width = MulDiv(width, windowDpi, 96);
        }
        list.SetColumnWidth(i, width);
        m_taskListSort.SetColumnSortType(i, columns[i].sort);
    }

    // Reduce the width of the last column so that a horizontal scrollbar won't
    // appear when the vertical scrollbar is visible.
    int lastColumn = ARRAYSIZE(columns) - 1;
    int scrollbarWidth =
        Functions::GetSystemMetricsForDpiWithFallback(SM_CXVSCROLL, windowDpi);
    list.SetColumnWidth(
        lastColumn, std::max(list.GetColumnWidth(lastColumn) - scrollbarWidth,
                             scrollbarWidth));

    m_taskListSort.SetSortColumn(0);

    // Fix tooltip not always on top.
    if (GetExStyle() & WS_EX_TOPMOST) {
        list.GetToolTips().SetWindowPos(
            HWND_TOPMOST, 0, 0, 0, 0, SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE);
    }
}

void CTaskManagerDlg::LoadTaskList() {
    m_taskListSort.SetRedraw(FALSE);

    auto redrawWhenDone = wil::scope_exit([this] {
        m_taskListSort.SetRedraw(TRUE);
        m_taskListSort.RedrawWindow(
            nullptr, nullptr,
            RDW_ERASE | RDW_FRAME | RDW_INVALIDATE | RDW_ALLCHILDREN);
    });

    PCWSTR metadataCategory = nullptr;
    switch (m_dialogOptions.dataSource) {
        case DataSource::kModStatus:
            metadataCategory = L"mod-status";
            break;

        case DataSource::kModTask:
            metadataCategory = L"mod-task";
            break;
    }

    auto metadataPath =
        StorageManager::GetInstance().GetModMetadataPath(metadataCategory);

    int firstItemIndex = m_taskListSort.GetItemCount();
    int itemIndex = firstItemIndex;

    int selectedIndex = m_taskListSort.GetSelectedIndex();
    bool isSelectionVisible = selectedIndex == -1
                                  ? false
                                  : m_taskListSort.IsItemVisible(selectedIndex);
    std::wstring selectedFilePath =
        selectedIndex == -1 ? std::wstring()
                            : reinterpret_cast<ListItemData*>(
                                  m_taskListSort.GetItemData(selectedIndex))
                                  ->filePath;

    if (std::filesystem::exists(metadataPath)) {
        for (const auto& p :
             std::filesystem::directory_iterator(metadataPath)) {
            if (!p.is_regular_file()) {
                continue;
            }

            try {
                if (!LoadTaskItemFromMetadataFile(p.path(), itemIndex)) {
                    VERBOSE(L"Didn't load %s", p.path().c_str());
                    continue;
                }

                if (selectedFilePath == p.path()) {
                    // Like SelectItem, but without EnsureVisible.
                    if (m_taskListSort.SetItemState(
                            itemIndex, LVIS_SELECTED | LVIS_FOCUSED,
                            LVIS_SELECTED | LVIS_FOCUSED)) {
                        m_taskListSort.SetSelectionMark(itemIndex);
                    }
                }

                itemIndex++;
            } catch (const std::exception& e) {
                LOG(L"Error handling %s: %S", p.path().c_str(), e.what());
            }
        }
    }

    // Remove old items only after adding new items to preserve the scroll
    // position.
    for (int i = 0; i < firstItemIndex; i++) {
        delete reinterpret_cast<ListItemData*>(m_taskListSort.GetItemData(0));
        m_taskListSort.DeleteItem(0);
    }

    m_taskListSort.DoSortItems(m_taskListSort.GetSortColumn(),
                               m_taskListSort.IsSortDescending());

    if (isSelectionVisible) {
        int newSelectedIndex = m_taskListSort.GetSelectedIndex();
        if (newSelectedIndex != -1) {
            m_taskListSort.EnsureVisible(newSelectedIndex, FALSE);
        }
    }
}

bool CTaskManagerDlg::LoadTaskItemFromMetadataFile(
    const std::filesystem::path& filePath,
    int itemIndex) {
    auto filenameParts =
        Functions::SplitString(filePath.filename().native(), L'_');
    if (filenameParts.size() != 4) {
        return false;
    }

    DWORD sessionManagerProcessId = std::stoul(filenameParts[0]);
    ULONGLONG sessionManagerProcessCreationTime = std::stoull(filenameParts[1]);
    if (sessionManagerProcessId != m_dialogOptions.sessionManagerProcessId ||
        sessionManagerProcessCreationTime !=
            m_dialogOptions.sessionManagerProcessCreationTime) {
        // Probably a stale file, try to remove.
        std::error_code ec;
        std::filesystem::remove(filePath, ec);
        return false;
    }

    DWORD targetProcessId = std::stoul(filenameParts[2]);

    auto& modName = filenameParts[3];

    FILETIME creationTime;
    std::wstring metadata = GetMetadataContent(filePath.c_str(), &creationTime);

    PCWSTR processName = metadata.c_str();
    PCWSTR status = L"";

    auto separator = metadata.find(L'|');
    if (separator != metadata.npos) {
        metadata[separator] = L'\0';
        status = metadata.c_str() + separator + 1;
    }

    AddItemToList(itemIndex, filePath.c_str(), modName.c_str(), processName,
                  targetProcessId, status, creationTime);
    return true;
}

void CTaskManagerDlg::AddItemToList(int itemIndex,
                                    PCWSTR filePath,
                                    PCWSTR mod,
                                    PCWSTR processName,
                                    DWORD processId,
                                    PCWSTR status,
                                    FILETIME creationTime) {
    std::wstring processNameFormatted = processName;
    bool isFrozen = IsProcessFrozen(processId);
    if (isFrozen) {
        processNameFormatted += L' ';
        processNameFormatted +=
            Functions::LoadStrFromRsrc(IDS_TASKDLG_PROCESS_SUSPENDED);
    }

    m_taskListSort.AddItem(itemIndex, 0, mod);
    m_taskListSort.AddItem(itemIndex, 1, processNameFormatted.c_str());
    m_taskListSort.AddItem(itemIndex, 2, std::to_wstring(processId).c_str());
    m_taskListSort.AddItem(itemIndex, 3, LocalizeStatus(status).c_str());

    // The process handle must be kept alive while the request is active.
    // Otherwise, a BSOD might occur in Windows 10.
    wil::unique_process_handle executionRequiredRequestProcess;
    wil::unique_handle executionRequiredRequest;
    if (Functions::IsWindowsVersionOrGreaterWithBuildNumber(10, 0, 0)) {
        executionRequiredRequestProcess.reset(
            OpenProcess(PROCESS_SET_LIMITED_INFORMATION, FALSE, processId));
        if (executionRequiredRequestProcess) {
            HRESULT hr = Functions::CreateExecutionRequiredRequest(
                executionRequiredRequestProcess.get(),
                executionRequiredRequest.put());
            if (FAILED(hr) || !executionRequiredRequest) {
                LOG(L"Failed to create execution required request: %08X", hr);
                executionRequiredRequest.reset();
                executionRequiredRequestProcess.reset();
            }
        }
    }

    auto* itemData = new ListItemData{
        .filePath = filePath,
        .processName = processName,
        .processId = processId,
        .creationTime = wil::filetime::to_int64(creationTime),
        .isFrozen = isFrozen,
        .executionRequiredRequestProcess =
            std::move(executionRequiredRequestProcess),
        .executionRequiredRequest = std::move(executionRequiredRequest),
    };
    m_taskListSort.SetItemData(itemIndex,
                               reinterpret_cast<DWORD_PTR>(itemData));
}

void CTaskManagerDlg::RefreshTaskList() {
    try {
        LoadTaskList();
    } catch (const std::exception& e) {
        if (!m_dialogOptions.autonomousMode) {
            ::MessageBoxA(m_hWnd, e.what(), "Failed to update data",
                          MB_ICONERROR);
        } else {
            LOG(L"%S", e.what());
        }
        DestroyWindow();
        return;
    }

    UpdateDialogAfterListUpdate();
}

void CTaskManagerDlg::UpdateTaskListProcessesStatus() {
    bool updated = false;

    int itemCount = m_taskListSort.GetItemCount();
    for (int i = 0; i < itemCount; i++) {
        auto* itemData =
            reinterpret_cast<ListItemData*>(m_taskListSort.GetItemData(i));

        bool isFrozen = IsProcessFrozen(itemData->processId);
        if (isFrozen == itemData->isFrozen) {
            continue;
        }

        itemData->isFrozen = isFrozen;

        std::wstring processNameFormatted = itemData->processName;
        if (isFrozen) {
            processNameFormatted += L' ';
            processNameFormatted +=
                Functions::LoadStrFromRsrc(IDS_TASKDLG_PROCESS_SUSPENDED);
        }

        m_taskListSort.SetItemText(i, 1, processNameFormatted.c_str());

        updated = true;
    }

    if (updated) {
        UpdateDialogAfterListUpdate();
    }
}

void CTaskManagerDlg::UpdateDialogAfterListUpdate() {
    if (!m_dialogOptions.autonomousMode) {
        return;
    }

    int itemCount = m_taskListSort.GetItemCount();
    if (itemCount == 0) {
        DestroyWindow();
        return;
    }

    bool allProcessesAreFrozen = true;
    for (int i = 0; i < itemCount; i++) {
        auto* itemData =
            reinterpret_cast<ListItemData*>(m_taskListSort.GetItemData(i));
        if (!itemData->isFrozen) {
            allProcessesAreFrozen = false;
            break;
        }
    }

    if (allProcessesAreFrozen) {
        if (m_showDlgPending) {
            KillTimer(Timer::kShowDlg);
            m_showDlgPending = false;
        }

        ShowWindow(SW_HIDE);
        return;
    }

    if (!IsWindowVisible()) {
        // Set timer to show the dialog. The delay is the defined amount of
        // delay in autonomousModeShowDelay, minus the earliest item age. This
        // is to avoid showing the dialog when items come and go - the delay
        // will always be updated and the dialog will never be shown.

        ULONGLONG earliestCreationTime = ULONGLONG_MAX;
        for (int i = 0; i < itemCount; i++) {
            auto* itemData =
                reinterpret_cast<ListItemData*>(m_taskListSort.GetItemData(i));
            ULONGLONG creationTime = itemData->creationTime;
            if (creationTime < earliestCreationTime) {
                earliestCreationTime = creationTime;
            }
        }

        ULONGLONG currentTime =
            wil::filetime::to_int64(wil::filetime::get_system_time());

        UINT delay = std::max(m_dialogOptions.autonomousModeShowDelay,
                              kAutonomousModeShowDelayMin);

        if (earliestCreationTime <= currentTime) {
            ULONGLONG msSinceEarliestCreationTime =
                wil::filetime::convert_100ns_to_msec(currentTime -
                                                     earliestCreationTime);

            if (msSinceEarliestCreationTime >= delay) {
                delay = 0;
            } else {
                delay -= static_cast<UINT>(msSinceEarliestCreationTime);
            }
        }

        SetTimer(Timer::kShowDlg, delay);
        m_showDlgPending = true;
    }
}
