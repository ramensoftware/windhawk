import { promptDevToolsInstall } from '@app/devToolsInstall';
import { isWireError } from '@app/feedback';
import {
  useCancelImportUserData,
  useGetInstalledMods,
  useImportUserData,
  useImportUserDataProgress,
} from '@app/webviewIPC';
import {
  type UserDataImportModOutcome,
  type UserDataImportSummary,
  type UserDataManifest,
} from '@app/webviewIPCMessages';
import { getDisplayModId, isLocalModId, testIdProps } from '@app/utils';
import {
  Alert,
  Badge,
  Button,
  List,
  Modal,
  Progress,
  Result,
  Switch,
  Tag,
} from 'antd';
import {
  type CSSProperties,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';

import {
  abortedOutcomeRows,
  type AppSettingsOutcomeStatus,
  type ImportOutcomeRow,
  orderOutcomesByModIds,
} from './importOutcomes';
import {
  buildSelection,
  importRowsFromManifest,
  initialImportState,
  isSelectionEmpty,
  type UserDataSelectionState,
} from './selection';
import { UserDataSelectionForm } from './UserDataSelectionForm';

// Fills the fixed-height dialog body so the mod list inside the form is the only
// scroll region; the surrounding warnings, options and badge stay pinned.
const Body = styled.div`
  display: flex;
  flex-direction: column;
  gap: 16px;
  flex: 1 1 auto;
  min-height: 0;
`;

const OptionRow = styled.div`
  display: flex;
  align-items: flex-start;
  gap: 12px;
`;

const OptionText = styled.div`
  flex: 1 1 auto;
`;

const OptionTitle = styled.div`
  font-weight: 600;
`;

const OptionDescription = styled.div`
  color: var(--whui-text-muted);
  font-size: 13px;
`;

const RunningWrapper = styled.div`
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 8px 0;
  flex: 1 1 auto;
  min-height: 0;
`;

const RunningStatus = styled.div`
  text-align: center;
  font-size: 15px;
`;

const RunningSub = styled.div`
  text-align: center;
  color: var(--whui-text-muted);
  font-size: 13px;
  min-height: 18px;
`;

// The done and failed phases render every outcome row with no inner scroll; the modal
// body scrolls the result as a whole when the list is long.
const OutcomeList = styled.div`
  border: 1px solid var(--whui-border);
  border-radius: 6px;
  background: var(--whui-card-background-color);
`;

// While the import runs, the live list fills the phase's leftover height and is its own
// scroller, shrinking with the window (min-height: 0, no floor) so the status and
// progress bar above it and the Cancel button below it stay in view.
const RunningOutcomeList = styled(OutcomeList)`
  flex: 1 1 auto;
  min-height: 0;
  overflow: auto;
`;

const NetworkBadgeRow = styled.div`
  border-top: 1px solid var(--whui-divider);
  padding-top: 12px;
`;

// A floor for the select-phase body so the mod list stays usable on short windows.
// Below this the min-height wins over the 70vh cap: the modal grows past the viewport
// and the whole modal scrolls (the antd wrap), the list scrolling within it - a double
// scroll, but the two are far apart (list vs. whole modal) and only on small windows.
const SELECT_BODY_MIN_HEIGHT = 600;

type ImportOptionsState = {
  noPrecompiled: boolean;
};

type Phase = 'select' | 'running' | 'done' | 'failed' | 'aborted';

interface Props {
  manifest: UserDataManifest;
  archive: string;
  onClose: () => void;
  // Called when the import has changed state on disk the caller may be showing, so it
  // can refresh: as soon as the app settings are applied (mid-run, ahead of the mod
  // loop) and again at any terminal outcome. Fires more than once per import, and the
  // refresh it triggers is expected to be idempotent.
  onImported?: () => void;
}

export function ImportModal({ manifest, archive, onClose, onImported }: Props) {
  const { t } = useTranslation();

  const rows = useMemo(() => importRowsFromManifest(manifest), [manifest]);
  // The archive's own mod order, which the host imports in and the running list streams
  // in; the result lists follow it too, so they read the way the run went rather than
  // the order the terminal summary happened to arrive in.
  const archiveModIds = useMemo(
    () => manifest.mods.map((mod) => mod.modId),
    [manifest]
  );
  const [state, setState] = useState<UserDataSelectionState>(() =>
    initialImportState(manifest, rows)
  );
  const [options, setOptions] = useState<ImportOptionsState>({
    noPrecompiled: false,
  });

  // The mods installed on this machine, so the dialog can flag which of the archive's
  // mods an import would overwrite. Import is always overwrite in the GUI.
  const [installedModIds, setInstalledModIds] = useState<Set<string>>(new Set());
  const { getInstalledMods } = useGetInstalledMods(
    useCallback((data) => {
      setInstalledModIds(new Set(Object.keys(data.installedMods)));
    }, [])
  );
  useEffect(() => {
    getInstalledMods({});
  }, [getInstalledMods]);

  // The trust warning is dismissible to reclaim its space; the dismissal is tracked
  // here so it stays closed if the select phase is re-entered (e.g. the dev-tools
  // retry path), which would otherwise remount the alert and reset it.
  const [trustWarningDismissed, setTrustWarningDismissed] = useState(false);

  const [phase, setPhase] = useState<Phase>('select');
  const [progress, setProgress] = useState<{
    total: number;
    index: number;
    modId: string;
    compileTarget?: string;
  } | null>(null);
  const [outcomes, setOutcomes] = useState<UserDataImportModOutcome[]>([]);
  const [summary, setSummary] = useState<UserDataImportSummary | null>(null);
  const [cancelRequested, setCancelRequested] = useState(false);
  // The latest app-settings progress marker the host streamed ('applying' then
  // 'applied'), or null when app settings were not part of the import or no marker has
  // arrived yet. Drives the "App settings" row's live status in the outcome list.
  const [appSettingsStatus, setAppSettingsStatus] = useState<
    'applying' | 'applied' | null
  >(null);

  // Refs mirror the accumulating outcomes and the cancel flag so the terminal reply
  // handler reads their latest values without being re-created every event.
  const outcomesRef = useRef<UserDataImportModOutcome[]>([]);
  const cancelRequestedRef = useRef(false);
  // Same reason: lets the [] -dep reply handler call the latest onImported.
  const onImportedRef = useRef(onImported);
  useEffect(() => {
    onImportedRef.current = onImported;
  }, [onImported]);

  useImportUserDataProgress(
    useCallback((data) => {
      if (data.item === 'appSettings') {
        // The app-settings step reports its own progress, ahead of the mods and with no
        // mod position; track it for the "App settings" row.
        setAppSettingsStatus(data.status);
        if (data.status === 'applied') {
          // The archive's app settings are on disk at this marker, which the host emits
          // before the mod loop - a loop that can run for minutes. Let the caller
          // refresh here rather than only at the terminal reply, so the imported
          // settings show up as they land (the hosts announce their own appUISettings
          // push on the same marker).
          onImportedRef.current?.();
        }
        return;
      }
      setProgress({
        total: data.total,
        index: data.index,
        modId: data.modId,
        compileTarget: data.compileTarget,
      });
      if (
        data.status === 'installed' ||
        data.status === 'skipped' ||
        data.status === 'failed'
      ) {
        const outcome: UserDataImportModOutcome = {
          modId: data.modId,
          status: data.status,
          message: data.message,
        };
        outcomesRef.current = [...outcomesRef.current, outcome];
        setOutcomes(outcomesRef.current);
      }
    }, [])
  );

  const { importUserData } = useImportUserData(
    useCallback((data) => {
      if (isWireError(data.error) && data.error.code === 'DEV_TOOLS_MISSING') {
        // The import fail-fasts before any change when a local compile is needed but
        // the development tools are missing. Raise the install-dev-tools prompt (as an
        // install/compile does) and return to the form so the user can retry.
        promptDevToolsInstall();
        setPhase('select');
        return;
      }
      // Past the fail-fast: the import ran and may have changed settings on disk (even a
      // canceled or failed one can be partial), so let the caller refresh its view.
      onImportedRef.current?.();
      if (data.succeeded) {
        // The terminal summary is authoritative; fall back to what progress
        // accumulated (e.g. a mock host that streams no progress).
        setSummary(
          data.summary ?? { mods: outcomesRef.current }
        );
        setPhase('done');
      } else if (cancelRequestedRef.current) {
        // A user cancel: the operation-level error is CANCELED (not surfaced). Show
        // the run as aborted, listing what completed before the cancel.
        setSummary({ mods: outcomesRef.current });
        setPhase('aborted');
      } else {
        // An operation-level failure; its error was already surfaced by the IPC layer.
        setPhase('failed');
      }
    }, [])
  );

  const { cancelImportUserData } = useCancelImportUserData(
    useCallback(() => {
      // The ack only tells us the cancel was accepted; the import's own terminal
      // reply still arrives and drives the phase.
    }, [])
  );

  const selectionEmpty = isSelectionEmpty(rows, state);

  // Selected mods that are already installed - importing them overwrites their current
  // settings and configuration. Drives the summary warning and the per-row markers.
  const overwriteModIds = useMemo(() => {
    const ids = new Set<string>();
    for (const mod of manifest.mods) {
      if (installedModIds.has(mod.modId) && state.perMod[mod.modId]?.included) {
        ids.add(mod.modId);
      }
    }
    return ids;
  }, [manifest, installedModIds, state]);

  // What network the import will use, derived from the selection (rather than an
  // opaque toggle). Source is fetched for any selected reference-only repository mod
  // (no embedded source); precompiled binaries are downloaded for repository mods
  // unless "Force local compilation" is on. Local mods embed their source and always
  // compile locally, so they never need the network.
  const network = useMemo(() => {
    const selectedRepoMods = manifest.mods.filter(
      (mod) => !mod.isLocal && state.perMod[mod.modId]?.included
    );
    return {
      needsSource: selectedRepoMods.some((mod) => !mod.hasSource),
      needsPrecompiled: !options.noPrecompiled && selectedRepoMods.length > 0,
    };
  }, [manifest, state, options.noPrecompiled]);

  const handleImport = () => {
    outcomesRef.current = [];
    cancelRequestedRef.current = false;
    setOutcomes([]);
    setSummary(null);
    setProgress(null);
    setAppSettingsStatus(null);
    setCancelRequested(false);
    setPhase('running');
    importUserData({
      archive,
      selection: buildSelection(rows, state),
      options: {
        // The GUI does not gate on a network-free restore; it surfaces the network
        // requirement instead (the badge), so offline is never forced here.
        offline: false,
        noPrecompiled: options.noPrecompiled,
        // The GUI always overwrites an already-installed mod (it warns up front which
        // mods that affects); the skip policy is CLI-only.
        onConflict: 'overwrite',
        // The GUI's explicit Import confirmation is the app-restart acknowledgment;
        // the host then restarts the Windhawk engine after import when the imported
        // settings require it, like saving advanced app settings does.
        confirmAppRestart: true,
      },
    });
  };

  const handleCancel = () => {
    cancelRequestedRef.current = true;
    setCancelRequested(true);
    cancelImportUserData({});
  };

  const running = phase === 'running';

  // How many mods this import processes: the selected rows, not the whole
  // manifest. The first progress event's authoritative total replaces it, so
  // this only seeds the counter before that event arrives.
  const selectedCount = rows.filter(
    (row) => state.perMod[row.modId]?.included
  ).length;

  // The select phase pins its chrome and lets only the mod list scroll (one scroll
  // region): a flex body capped at 70vh, no body scroll. On a window too short for the
  // min-height floor, the floor wins over the cap so the modal grows past the viewport
  // and the whole modal scrolls instead. The running phase is also a capped flex column
  // but has no floor: its live list shrinks with the window rather than forcing a scroll.
  // The result phases size to content and the body scrolls the outcome list as a whole.
  const maxBodyHeight = CSS.supports('height: 100dvh') ? '70dvh' : '70vh';
  const bodyStyle: CSSProperties =
    phase === 'select'
      ? {
          display: 'flex',
          flexDirection: 'column',
          minHeight: SELECT_BODY_MIN_HEIGHT,
          maxHeight: maxBodyHeight,
          overflow: 'hidden',
        }
      : phase === 'running'
        ? {
            display: 'flex',
            flexDirection: 'column',
            maxHeight: maxBodyHeight,
            overflow: 'hidden',
          }
        : { maxHeight: maxBodyHeight, overflow: 'auto' };

  const total = progress?.total ?? selectedCount;
  const completed = outcomes.length;
  const percent = total > 0 ? Math.round((completed / total) * 100) : 0;

  // The "App settings" row shown above the mods in the progress and result lists, or
  // undefined when app settings were not part of this import. While running it mirrors
  // the live marker (pending until the first one). In a terminal phase it is 'applied'
  // when the host reported the apply (app settings run first, so a cancel that reached
  // any mod already applied them), else 'aborted'.
  const appSettingsRow: AppSettingsOutcomeStatus | undefined = !state.appSettings
    ? undefined
    : phase === 'running'
      ? appSettingsStatus ?? 'pending'
      : phase === 'done'
        ? 'applied'
        : phase === 'aborted' || phase === 'failed'
          ? appSettingsStatus === 'applied'
            ? 'applied'
            : 'aborted'
          : undefined;

  const footer = (() => {
    switch (phase) {
      case 'running':
        return [
          <Button
            key="cancel"
            danger
            disabled={cancelRequested}
            data-testid="import-abort"
            onClick={handleCancel}
          >
            {cancelRequested
              ? t('settings.userData.import.cancelling')
              : t('general.actions.cancel')}
          </Button>,
        ];
      case 'done':
      case 'failed':
      case 'aborted':
        return [
          <Button
            key="close"
            type="primary"
            data-testid="import-close"
            onClick={onClose}
          >
            {t('general.actions.close')}
          </Button>,
        ];
      default:
        return [
          <Button key="cancel" data-testid="import-cancel" onClick={onClose}>
            {t('general.actions.cancel')}
          </Button>,
          <Button
            key="import"
            type="primary"
            disabled={selectionEmpty}
            data-testid="import-confirm"
            onClick={handleImport}
          >
            {t('settings.userData.import.importButton')}
          </Button>,
        ];
    }
  })();

  return (
    <Modal
      open
      title={t('settings.userData.import.title')}
      onCancel={running ? undefined : onClose}
      closable={!running}
      maskClosable={false}
      width={620}
      centered
      bodyStyle={bodyStyle}
      wrapProps={testIdProps('import-modal')}
      footer={footer}
    >
      {phase === 'select' && (
        <Body>
          {!trustWarningDismissed && (
            <Alert
              type="warning"
              showIcon
              closable
              afterClose={() => setTrustWarningDismissed(true)}
              message={t('settings.userData.import.trustTitle')}
              description={t('settings.userData.import.trustDescription')}
            />
          )}
          <UserDataSelectionForm
            rows={rows}
            state={state}
            onChange={setState}
            appSettingsAvailable={manifest.hasAppSettings}
            overwriteModIds={overwriteModIds}
          />
          {overwriteModIds.size > 0 && (
            <Alert
              type="warning"
              showIcon
              message={t('settings.userData.import.overwriteWarning', {
                count: overwriteModIds.size,
              })}
            />
          )}
          <ImportOptions options={options} onChange={setOptions} />
          <NetworkBadgeRow>
            <NetworkBadge
              needsSource={network.needsSource}
              needsPrecompiled={network.needsPrecompiled}
            />
          </NetworkBadgeRow>
        </Body>
      )}

      {phase === 'running' && (
        <RunningWrapper>
          <RunningStatus>
            {t('settings.userData.import.runningStatus', {
              current: Math.min((progress?.index ?? 0) + 1, total),
              total,
            })}
          </RunningStatus>
          <RunningSub>
            {progress?.modId
              ? progress.compileTarget
                ? t('settings.userData.import.compilingFor', {
                    modId: getDisplayModId(progress.modId),
                    target: progress.compileTarget,
                  })
                : getDisplayModId(progress.modId)
              : ''}
          </RunningSub>
          <Progress percent={percent} status="active" />
          {(outcomes.length > 0 || appSettingsRow) && (
            <ProgressOutcomeList
              outcomes={outcomes}
              appSettings={appSettingsRow}
            />
          )}
        </RunningWrapper>
      )}

      {phase === 'done' && (
        <ImportSummaryView
          summary={summary}
          modIds={archiveModIds}
          appSettings={appSettingsRow}
        />
      )}

      {phase === 'aborted' && (
        <ImportAbortedView
          outcomes={abortedOutcomeRows(archiveModIds, state, summary)}
          appSettings={appSettingsRow}
        />
      )}

      {phase === 'failed' && (
        <Result
          status="error"
          title={t('settings.userData.import.failedTitle')}
          subTitle={t('settings.userData.import.failedSubtitle')}
        >
          {(outcomes.length > 0 || appSettingsRow) && (
            <OutcomeList>
              <OutcomeItems outcomes={outcomes} appSettings={appSettingsRow} />
            </OutcomeList>
          )}
        </Result>
      )}
    </Modal>
  );
}

function ImportOptions({
  options,
  onChange,
}: {
  options: ImportOptionsState;
  onChange: (options: ImportOptionsState) => void;
}) {
  const { t } = useTranslation();
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
      <OptionRow>
        <OptionText>
          <OptionTitle>
            {t('settings.userData.import.noPrecompiledTitle')}
          </OptionTitle>
          <OptionDescription>
            {t('settings.userData.import.noPrecompiledDescription')}
          </OptionDescription>
        </OptionText>
        <Switch
          checked={options.noPrecompiled}
          onChange={(checked) =>
            onChange({ ...options, noPrecompiled: checked })
          }
        />
      </OptionRow>
    </div>
  );
}

// A status badge stating whether the import needs the network, and for what, derived
// from the selection and the compile options - so the user reads the consequence
// rather than reasoning about a network-free toggle.
function NetworkBadge({
  needsSource,
  needsPrecompiled,
}: {
  needsSource: boolean;
  needsPrecompiled: boolean;
}) {
  const { t } = useTranslation();
  const needsNetwork = needsSource || needsPrecompiled;
  const key = !needsNetwork
    ? 'none'
    : needsSource && needsPrecompiled
      ? 'sourceAndPrecompiled'
      : needsSource
        ? 'source'
        : 'precompiled';
  return (
    <Badge
      color={needsNetwork ? 'blue' : 'green'}
      text={t(`settings.userData.import.network.${key}`)}
    />
  );
}

// The live outcome list while the import runs, streaming a row per processed mod. It
// follows the newest row as long as the view sits at the bottom; once the user scrolls
// up to read an earlier row, following stops until they scroll back down, so new rows
// never yank the view away from what they are reading.
function ProgressOutcomeList({
  outcomes,
  appSettings,
}: {
  outcomes: UserDataImportModOutcome[];
  appSettings?: AppSettingsOutcomeStatus;
}) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const stuckToBottom = useRef(true);

  const handleScroll = useCallback(() => {
    const el = scrollRef.current;
    if (!el) {
      return;
    }
    // A few px of slack absorbs sub-pixel rounding so resting at the bottom still counts.
    stuckToBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight <= 4;
  }, []);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el && stuckToBottom.current) {
      el.scrollTop = el.scrollHeight;
    }
  }, [outcomes]);

  return (
    <RunningOutcomeList ref={scrollRef} onScroll={handleScroll}>
      <OutcomeItems outcomes={outcomes} appSettings={appSettings} />
    </RunningOutcomeList>
  );
}

// One row of the outcome list: a mod, or the app-settings step. App settings sit first
// (above the mods), matching the selection form.
type OutcomeListItem =
  | { kind: 'appSettings'; status: AppSettingsOutcomeStatus }
  | { kind: 'mod'; outcome: ImportOutcomeRow };

function OutcomeItems({
  outcomes,
  appSettings,
}: {
  outcomes: ImportOutcomeRow[];
  appSettings?: AppSettingsOutcomeStatus;
}) {
  const { t } = useTranslation();
  const items: OutcomeListItem[] = [
    ...(appSettings ? [{ kind: 'appSettings' as const, status: appSettings }] : []),
    ...outcomes.map((outcome) => ({ kind: 'mod' as const, outcome })),
  ];
  return (
    <List
      size="small"
      dataSource={items}
      renderItem={(item) =>
        item.kind === 'appSettings' ? (
          <List.Item
            data-testid="import-outcome-app-settings"
            data-status={item.status}
          >
            <List.Item.Meta title={t('settings.userData.appSettings')} />
            <AppSettingsOutcomeTag status={item.status} />
          </List.Item>
        ) : (
          <List.Item
            data-testid="import-outcome"
            data-mod-id={item.outcome.modId}
            data-status={item.outcome.status}
          >
            <List.Item.Meta
              title={
                <>
                  {getDisplayModId(item.outcome.modId)}
                  {/* The display id strips the local@ prefix, so the tag keeps a
                      local mod distinguishable from a same-named repository mod,
                      like the selection list does. */}
                  {isLocalModId(item.outcome.modId) && (
                    <Tag color="blue" style={{ marginLeft: 8 }}>
                      {t('settings.userData.local')}
                    </Tag>
                  )}
                </>
              }
              description={item.outcome.message}
            />
            <OutcomeTag status={item.outcome.status} />
          </List.Item>
        )
      }
    />
  );
}

function OutcomeTag({ status }: { status: ImportOutcomeRow['status'] }) {
  const { t } = useTranslation();
  // installed is a win, failed is an error; skipped (conflict skip) and aborted (a mod
  // the cancel never reached) are both neutral, not-applied states.
  const color =
    status === 'installed' ? 'success' : status === 'failed' ? 'error' : 'default';
  return <Tag color={color}>{t(`settings.userData.import.modStatus.${status}`)}</Tag>;
}

function AppSettingsOutcomeTag({
  status,
}: {
  status: AppSettingsOutcomeStatus;
}) {
  const { t } = useTranslation();
  // applied is a win; applying is in-flight; pending (not yet reached) and aborted (a
  // canceled/failed import that never applied them) are both neutral.
  const color =
    status === 'applied'
      ? 'success'
      : status === 'applying'
        ? 'processing'
        : 'default';
  return (
    <Tag color={color}>
      {t(`settings.userData.import.appSettingsStatus.${status}`)}
    </Tag>
  );
}

function ImportSummaryView({
  summary,
  modIds,
  appSettings,
}: {
  summary: UserDataImportSummary | null;
  modIds: string[];
  appSettings?: AppSettingsOutcomeStatus;
}) {
  const { t } = useTranslation();
  // Show the outcomes in the archive's order, not the order the host happened to report
  // them in, so the summary reads the same way the progress list did.
  const mods = orderOutcomesByModIds(modIds, summary?.mods ?? []);
  const failedCount = mods.filter((m) => m.status === 'failed').length;
  const installedCount = mods.filter((m) => m.status === 'installed').length;
  const skippedCount = mods.filter((m) => m.status === 'skipped').length;

  return (
    <Result
      status={failedCount > 0 ? 'warning' : 'success'}
      title={
        failedCount > 0
          ? t('settings.userData.import.doneWithFailuresTitle')
          : t('settings.userData.import.doneTitle')
      }
      subTitle={t('settings.userData.import.doneSubtitle', {
        installed: installedCount,
        skipped: skippedCount,
        failed: failedCount,
      })}
    >
      <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
        {(mods.length > 0 || appSettings) && (
          <OutcomeList>
            <OutcomeItems outcomes={mods} appSettings={appSettings} />
          </OutcomeList>
        )}
      </div>
    </Result>
  );
}

// The result of a canceled import: every selected mod, with the ones the cancel never
// reached tagged "Aborted", so the count reconciles (a 28-mod cancel that finished one
// reads 1 installed + 27 aborted rather than dropping the 27).
function ImportAbortedView({
  outcomes,
  appSettings,
}: {
  outcomes: ImportOutcomeRow[];
  appSettings?: AppSettingsOutcomeStatus;
}) {
  const { t } = useTranslation();
  const total = outcomes.length;
  const done = outcomes.filter((o) => o.status !== 'aborted').length;

  return (
    <Result
      status="warning"
      title={t('settings.userData.import.abortedTitle')}
      subTitle={t('settings.userData.import.abortedSubtitle', { done, total })}
    >
      <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
        {(outcomes.length > 0 || appSettings) && (
          <OutcomeList>
            <OutcomeItems outcomes={outcomes} appSettings={appSettings} />
          </OutcomeList>
        )}
      </div>
    </Result>
  );
}
