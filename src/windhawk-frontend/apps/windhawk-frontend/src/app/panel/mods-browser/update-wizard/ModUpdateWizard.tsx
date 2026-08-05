import { useNavigationBlock } from '@app/navigationBlock';
import useModalClose from '@app/panel/shared/useModalClose';
import { getDisplayModId, testIdProps } from '@app/utils';
import { useCancelInstallMod, useInstallMod } from '@app/webviewIPC';
import type { InstallModReplyData } from '@app/webviewIPCMessages';
import { faArrowRightLong } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Button, List, Modal, Progress, Result, Tag } from 'antd';
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

import ModUpdateDetailsModal from './ModUpdateDetailsModal';
import ModUpdateList, { type UpdatableMod } from './ModUpdateList';
import {
  countOutcomes,
  finalRows,
  type ModUpdateOutcome,
  type ModUpdateStatus,
} from './updateRun';
import {
  type ModUpdateSource,
  useModUpdateSources,
} from './useModUpdateSources';

export type InstalledModDetails = NonNullable<
  InstallModReplyData['installedModDetails']
>;

// Fills the fixed-height dialog body so the mod list is the only scroll region.
const Body = styled.div`
  display: flex;
  flex-direction: column;
  gap: 16px;
  flex: 1 1 auto;
  min-height: 0;
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

const OutcomeList = styled.div`
  border: 1px solid var(--whui-border);
  border-radius: 6px;
  background: var(--whui-card-background-color);
`;

// While the run is going, the live list fills the phase's leftover height and is
// its own scroller, so the status and progress above it and the Cancel below it
// stay in view.
const RunningOutcomeList = styled(OutcomeList)`
  flex: 1 1 auto;
  min-height: 0;
  overflow: auto;
`;

// The version a mod moves from and the one it moves to. Not a word in it, so it is
// composed here rather than through a translation key nobody could translate.
//
// A version pair is a progression, like a number line, so it reads left to right
// whichever way the app is running - and the arrow, being a neutral run between two
// number runs, would be handed back reordered by an RTL paragraph otherwise. The
// isolate keeps that from leaking the other way: the box still sits where the
// surrounding direction puts it.
const VersionChange = styled.span`
  display: inline-flex;
  align-items: center;
  gap: 6px;
  direction: ltr;
  unicode-bidi: isolate;
  color: var(--whui-text-muted);
`;

// Slightly under the text it sits between, so it separates the two versions
// without competing with them.
const VersionArrow = styled(FontAwesomeIcon)`
  font-size: 0.85em;
`;

// The version the mod ends up on, which is the half of the pair worth reading. The
// two text tokens are close enough in the light theme that the weight is what
// carries this, not the color.
const VersionTo = styled.span`
  color: var(--whui-text-secondary);
  font-weight: 600;
`;

// A floor for the select-phase body, so the list stays usable on short windows.
// Below it the floor wins over the 70vh cap and the whole modal scrolls instead.
const SELECT_BODY_MIN_HEIGHT = 420;

// `failed` is a run that could not start or could not go on at all; a mod that
// failed to install is a row of `done`, not this.
type Phase = 'select' | 'running' | 'done' | 'aborted' | 'failed';

interface Props {
  // The updatable mods, in the order they are listed and updated. Read once, at
  // mount: see the snapshot below.
  mods: UpdatableMod[];
  onClose: () => void;
  // Reported per mod as the run goes, not once at the end, so the grid behind
  // loses each tag as it is earned and a cancelled run leaves it consistent with
  // what actually happened.
  onModUpdated: (modId: string, details: InstalledModDetails) => void;
}

/**
 * Updating several mods in one pass: pick them, see what each update contains, and
 * run the installs one after another with a progress report and a per-mod summary.
 *
 * It mounts its own `useInstallMod` rather than borrowing the mods browser's,
 * whose blocking progress modal would cover this one for every mod in the run, and
 * reports each success back up so the browser's own state stays correct.
 *
 * Render it only while it is open: each run starts from a fresh mount, which is
 * what keeps a second run from inheriting the first one's position.
 */
export function ModUpdateWizard({
  mods: modsProp,
  onClose,
  onModUpdated,
}: Props) {
  const { t } = useTranslation();

  const { open, close, afterClose } = useModalClose(onClose);

  // The list the wizard runs off for its whole life. The caller derives it from
  // the mods that have an update waiting, and `onModUpdated` clears that flag per
  // mod, so a live prop would drop each mod as the run succeeded on it - taking
  // its name out of the summary that is the only account of what the run did, and
  // rewriting the rows under a running install for anything else that writes to
  // the same state.
  const [mods] = useState(modsProp);

  const modIds = useMemo(() => mods.map((mod) => mod.modId), [mods]);
  const { sources, retry, loadInstalledSource } = useModUpdateSources(modIds);

  const [selected, setSelected] = useState<Set<string>>(() => new Set(modIds));
  const [phase, setPhase] = useState<Phase>('select');
  const [outcomes, setOutcomes] = useState<ModUpdateOutcome[]>([]);
  const [runOrder, setRunOrder] = useState<string[]>([]);
  const [runIndex, setRunIndex] = useState(0);
  const [cancelPending, setCancelPending] = useState(false);
  // The mod whose detail modal is open over the wizard, if any.
  const [detailsModId, setDetailsModId] = useState<string | null>(null);

  // The run is a state machine over the install reply handler rather than a loop:
  // `useInstallMod` posts and calls back, so the position, the sources it was
  // started with and the outcomes so far are held where that handler reads them.
  const runOrderRef = useRef<string[]>([]);
  const runSourcesRef = useRef<Record<string, string>>({});
  const runIndexRef = useRef(0);
  const outcomesRef = useRef<ModUpdateOutcome[]>([]);
  const cancelRequestedRef = useRef(false);
  // The ack the cancel in flight is waiting on; see useCancelModOperation for what
  // its `succeeded` means.
  const ackRef = useRef<(succeeded: boolean) => void>();
  const releaseAck = useCallback((succeeded: boolean) => {
    ackRef.current?.(succeeded);
    ackRef.current = undefined;
  }, []);
  // Same reason as the refs above: lets the []-dep reply handler call the latest
  // callback without being re-created.
  const onModUpdatedRef = useRef(onModUpdated);
  useEffect(() => {
    onModUpdatedRef.current = onModUpdated;
  }, [onModUpdated]);

  const recordOutcome = useCallback(
    (modId: string, status: ModUpdateStatus) => {
      outcomesRef.current = [...outcomesRef.current, { modId, status }];
      setOutcomes(outcomesRef.current);
    },
    []
  );

  const finish = useCallback((next: Phase) => {
    setCancelPending(false);
    setPhase(next);
  }, []);

  // Set from the effect below, because the handler that calls it is created before
  // the step it drives.
  const advanceRef = useRef<() => void>(() => undefined);

  const { installMod, installModPending } = useInstallMod(
    useCallback(
      (data) => {
        const { modId, installedModDetails } = data;
        if (data.uiMissing) {
          // A local compile with the development tools absent: this install did not
          // run, and none of the remaining ones would either. The hook has already
          // raised the install prompt.
          //
          // Nothing done yet means the whole run is still ahead, so it goes back to
          // the selection to be started again once the tools are there - as the
          // import dialog does, which can always do this because it fail-fasts
          // before touching anything. Here a mod part way in can be the first to
          // need a local compile, and what the run did up to it is only on this
          // screen, so that case reports instead.
          finish(outcomesRef.current.length === 0 ? 'select' : 'failed');
          return;
        }
        if (installedModDetails) {
          recordOutcome(modId, 'updated');
          onModUpdatedRef.current(modId, installedModDetails);
        } else if (cancelRequestedRef.current) {
          // A cancelled install still replies, with null details.
          recordOutcome(modId, 'aborted');
        } else {
          // Null details is all a failed install carries - neither host attaches a
          // reason, having sent it to the compiler output window - so the row says
          // that it failed and the run moves on.
          recordOutcome(modId, 'failed');
        }
        advanceRef.current();
      },
      [finish, recordOutcome]
    )
  );

  const postInstall = useCallback(() => {
    const modId = runOrderRef.current[runIndexRef.current];
    setRunIndex(runIndexRef.current);
    installMod({ modId, modSource: runSourcesRef.current[modId] });
  }, [installMod]);

  const advance = useCallback(() => {
    runIndexRef.current += 1;
    if (cancelRequestedRef.current) {
      finish('aborted');
      return;
    }
    if (runIndexRef.current >= runOrderRef.current.length) {
      finish('done');
      return;
    }
    postInstall();
  }, [finish, postInstall]);

  useEffect(() => {
    advanceRef.current = advance;
  }, [advance]);

  const { cancelInstallMod } = useCancelInstallMod(
    useCallback((data) => releaseAck(data.succeeded), [releaseAck])
  );

  // The host's reply is not the only end a cancel can meet, the same two ends
  // useCancelModOperation answers: the install it names can reach its own first,
  // and the wizard can go away with the cancel still on the wire. The reply is
  // then moot or has nowhere to land, and the waiter below would keep waiting.
  // Answered here as taken up - not a claim about what the host did, but what
  // both cases leave: no install to go on offering a cancel for.
  useEffect(() => {
    if (!installModPending) {
      releaseAck(true);
    }
  }, [installModPending, releaseAck]);
  useEffect(() => () => releaseAck(true), [releaseAck]);

  // The selected mods that have something to install, each with the source to
  // install, in the order they are listed. The run's order and its sources are
  // both taken from this one pass, so it cannot name a mod it has no source for.
  // A source landing is what makes a row `ready`, so asking for both drops
  // nothing the status alone would keep; it is the two not being able to
  // disagree that is worth having.
  const selectedReady = mods.flatMap((mod) => {
    const source = sources[mod.modId];
    return selected.has(mod.modId) && source?.status === 'ready' && source.source
      ? [{ modId: mod.modId, source: source.source }]
      : [];
  });
  // A failed row cannot be updated and is excluded from the count; a row still
  // loading counts, and is what the Update button waits for.
  const selectedCount = mods.filter(
    (mod) => selected.has(mod.modId) && sources[mod.modId]?.status !== 'failed'
  ).length;
  const updateDisabled =
    selectedCount === 0 || selectedReady.length !== selectedCount;

  const handleUpdate = () => {
    if (updateDisabled) {
      return;
    }

    // The sources are copied here rather than read as the run goes: the run
    // installs what the user reviewed, whatever a later fetch reports.
    const runSources: Record<string, string> = {};
    for (const { modId, source } of selectedReady) {
      runSources[modId] = source;
    }
    const order = selectedReady.map((entry) => entry.modId);

    runOrderRef.current = order;
    runSourcesRef.current = runSources;
    runIndexRef.current = 0;
    outcomesRef.current = [];
    cancelRequestedRef.current = false;

    setOutcomes([]);
    setRunOrder(order);
    setCancelPending(false);
    setPhase('running');
    postInstall();
  };

  const handleCancel = async () => {
    // Set whichever way the ack goes, so the run stops after the mod in flight
    // even when the host had nothing to signal - which can mean the cancel got
    // there before the install was registered, leaving it running.
    cancelRequestedRef.current = true;
    setCancelPending(true);

    const modId = runOrderRef.current[runIndexRef.current];
    if (!modId) {
      // Nothing in flight to name, so there is nothing to ask the host for.
      setCancelPending(false);
      return;
    }

    const ack = new Promise<boolean>((resolve) => {
      ackRef.current = resolve;
    });
    cancelInstallMod({ modId });
    if (!(await ack)) {
      // Nothing was signaled, so there is still something to ask for.
      setCancelPending(false);
    }
  };

  const handleToggle = (modId: string, checked: boolean) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (checked) {
        next.add(modId);
      } else {
        next.delete(modId);
      }
      return next;
    });
  };

  const handleToggleAll = (checked: boolean) => {
    setSelected(
      checked
        ? new Set(
            mods
              .map((mod) => mod.modId)
              .filter((modId) => sources[modId]?.status !== 'failed')
          )
        : new Set()
    );
  };

  // Held against a route change for as long as it is open, which the import dialog
  // does from the running phase on. Its selection phase is worth the same, holding
  // the repository source of every listed mod: a fetch per mod, thrown away with
  // the dialog and made again from scratch on the way back in.
  useNavigationBlock(open);

  const running = phase === 'running';
  const resultRows = finalRows(runOrder, outcomes);
  const counts = countOutcomes(resultRows);
  const completed = outcomes.length;
  const total = runOrder.length;
  const percent = total > 0 ? Math.round((completed / total) * 100) : 0;
  const currentMod = mods.find((mod) => mod.modId === runOrder[runIndex]);

  // The select and running phases pin their chrome and let one region scroll,
  // capped at 70vh; the result phases size to content and scroll as a whole.
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
      : running
        ? {
            display: 'flex',
            flexDirection: 'column',
            maxHeight: maxBodyHeight,
            overflow: 'hidden',
          }
        : { maxHeight: maxBodyHeight, overflow: 'auto' };

  const footer = (() => {
    switch (phase) {
      case 'running':
        return [
          <Button
            key="cancel"
            danger
            disabled={cancelPending}
            data-testid="mod-updates-abort"
            onClick={handleCancel}
          >
            {cancelPending
              ? t('general.status.canceling')
              : t('general.actions.cancel')}
          </Button>,
        ];
      case 'select':
        return [
          <Button key="cancel" data-testid="mod-updates-cancel" onClick={close}>
            {t('general.actions.cancel')}
          </Button>,
          <Button
            key="update"
            type="primary"
            disabled={updateDisabled}
            data-testid="mod-updates-confirm"
            onClick={handleUpdate}
          >
            {t('modUpdates.updateButton', { count: selectedCount })}
          </Button>,
        ];
      default:
        return [
          <Button
            key="close"
            type="primary"
            data-testid="mod-updates-close"
            onClick={close}
          >
            {t('general.actions.close')}
          </Button>,
        ];
    }
  })();

  const detailsMod = mods.find((mod) => mod.modId === detailsModId);

  return (
    <>
      <Modal
        open={open}
        afterClose={afterClose}
        title={t('modUpdates.title')}
        onCancel={running ? undefined : close}
        closable={!running}
        // Never closed by the mask, as the import dialog is not. Even the
        // select phase has more behind it than a selection: the repository source
        // of every listed mod, fetched once and thrown away with the dialog. Once
        // the run has started the mask is what keeps the grid behind it out of
        // reach, and once it has finished a stray click there would take the
        // report of what happened with it.
        maskClosable={false}
        width={720}
        centered
        bodyStyle={bodyStyle}
        wrapProps={testIdProps('mod-updates-modal')}
        footer={footer}
      >
        {phase === 'select' && (
          <Body>
            <ModUpdateList
              mods={mods}
              sources={sources}
              selected={selected}
              onToggle={handleToggle}
              onToggleAll={handleToggleAll}
              onRetry={retry}
              onOpenDetails={setDetailsModId}
            />
          </Body>
        )}

        {running && (
          <RunningWrapper>
            <RunningStatus>
              {t('modUpdates.runningStatus', {
                current: Math.min(runIndex + 1, total),
                total,
              })}
            </RunningStatus>
            <RunningSub>{currentMod?.name ?? ''}</RunningSub>
            <Progress percent={percent} status="active" />
            {outcomes.length > 0 && (
              <RunningProgressList
                rows={outcomes}
                mods={mods}
                sources={sources}
              />
            )}
          </RunningWrapper>
        )}

        {phase === 'done' && (
          <Result
            status={counts.failed > 0 ? 'warning' : 'success'}
            title={
              counts.failed > 0
                ? t('modUpdates.doneWithFailuresTitle')
                : t('modUpdates.doneTitle')
            }
            subTitle={t('modUpdates.doneSubtitle', {
              updated: counts.updated,
              failed: counts.failed,
            })}
          >
            <OutcomeList>
              <OutcomeItems rows={resultRows} mods={mods} sources={sources} />
            </OutcomeList>
          </Result>
        )}

        {phase === 'aborted' && (
          <Result
            status="warning"
            title={t('modUpdates.abortedTitle')}
            // `done` is how far the run got, failures included - the same thing it
            // counts in the import dialog's aborted subtitle, so the two sentences
            // cannot be read against each other and mean different numbers. Which
            // of those the run updated is what the rows below say.
            subTitle={t('modUpdates.abortedSubtitle', {
              done: counts.total - counts.aborted,
              total: counts.total,
            })}
          >
            <OutcomeList>
              <OutcomeItems rows={resultRows} mods={mods} sources={sources} />
            </OutcomeList>
          </Result>
        )}

        {phase === 'failed' && (
          <Result
            status="error"
            title={t('modUpdates.failedTitle')}
            subTitle={t('modUpdates.failedSubtitle')}
          >
            {counts.total > 0 && (
              <OutcomeList>
                <OutcomeItems rows={resultRows} mods={mods} sources={sources} />
              </OutcomeList>
            )}
          </Result>
        )}
      </Modal>

      {detailsMod && (
        <ModUpdateDetailsModal
          mod={detailsMod}
          source={sources[detailsMod.modId]}
          onLoadInstalledSource={loadInstalledSource}
          onClose={() => setDetailsModId(null)}
        />
      )}
    </>
  );
}

/**
 * The live list while the run goes, streaming a row per mod as it is answered. It
 * follows the newest row as long as the view sits at the bottom; once the user
 * scrolls up to read an earlier row, following stops until they scroll back down,
 * so new rows never yank the view away from what they are reading.
 *
 * The same list, and the same bargain, as the import dialog's.
 */
function RunningProgressList(props: {
  rows: ModUpdateOutcome[];
  mods: UpdatableMod[];
  sources: Record<string, ModUpdateSource>;
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
  }, [props.rows]);

  return (
    <RunningOutcomeList ref={scrollRef} onScroll={handleScroll}>
      <OutcomeItems {...props} />
    </RunningOutcomeList>
  );
}

function OutcomeItems({
  rows,
  mods,
  sources,
}: {
  rows: ModUpdateOutcome[];
  mods: UpdatableMod[];
  sources: Record<string, ModUpdateSource>;
}) {
  const byModId = new Map(mods.map((mod) => [mod.modId, mod] as const));
  return (
    <List
      size="small"
      dataSource={rows}
      renderItem={(row) => {
        const mod = byModId.get(row.modId);
        const from = mod?.installedVersion;
        const to = sources[row.modId]?.version;
        return (
          <List.Item
            data-testid="mod-update-outcome"
            data-mod-id={row.modId}
            data-status={row.status}
          >
            <List.Item.Meta
              // The name, with the id behind it - the same way the select list
              // names a mod, and the only place the id is still available to a
              // user reading the report.
              title={<span title={getDisplayModId(row.modId)}>{mod?.name ?? row.modId}</span>}
              // The move this row is making, which is what the user selected it
              // for and what the summary would otherwise drop.
              description={
                from && to ? (
                  // The pair is on the element as well as in it: a test that reads
                  // the versions off the attributes does not have to be rewritten
                  // every time this line is dressed differently.
                  <VersionChange
                    data-testid="mod-update-version-change"
                    data-from={from}
                    data-to={to}
                  >
                    <span>{from}</span>
                    <VersionArrow icon={faArrowRightLong} />
                    <VersionTo>{to}</VersionTo>
                  </VersionChange>
                ) : undefined
              }
            />
            <OutcomeTag status={row.status} />
          </List.Item>
        );
      }}
    />
  );
}

function OutcomeTag({ status }: { status: ModUpdateStatus }) {
  const { t } = useTranslation();
  // updated is a win, failed is an error; aborted (a mod the cancel never reached)
  // is neutral, a not-applied state.
  const color =
    status === 'updated' ? 'success' : status === 'failed' ? 'error' : 'default';
  return <Tag color={color}>{t(`modUpdates.modStatus.${status}`)}</Tag>;
}

export default ModUpdateWizard;
