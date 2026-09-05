import { useNavigationBlock } from '@app/navigationBlock';
import useModalClose from '@app/panel/shared/useModalClose';
import { getDisplayModId, testIdProps } from '@app/utils';
import { useCancelInstallMod, useInstallMod } from '@app/webviewIPC';
import type { InstalledModDetails } from '@app/webviewIPCMessages';
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
import ModUpdateRunModal from './ModUpdateRunModal';
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
import VersionChange from './VersionChange';

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
  //
  // `profileFieldsKnown` is false where the host reported the update it landed
  // AND an error: it could not read the mod back afterwards, so the details'
  // profile-held fields are its stand-ins rather than answers.
  onModUpdated: (
    modId: string,
    details: InstalledModDetails,
    profileFieldsKnown: boolean
  ) => void;
}

/**
 * Updating several mods in one pass: pick them, see what each update contains, and
 * run the installs one after another with a progress report and a per-mod summary.
 *
 * Or one at a time, from the modal that mod's update is read in. That run reports
 * over the list rather than in place of it, and the row it updated says so, so a
 * user who is weighing the updates one by one is not returned to a summary and an
 * empty screen after the first one.
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
  // Which run is going: a batch takes the dialog through the phases below, a
  // single mod's leaves it on its list. See `singleRun` where they part.
  const [runScope, setRunScope] = useState<'batch' | 'single'>('batch');
  // The mods updated so far, which the list would otherwise have no way of
  // knowing: it runs on a snapshot, so a row goes on offering an update the mod
  // has already taken.
  const [updatedModIds, setUpdatedModIds] = useState<Set<string>>(
    () => new Set()
  );
  const [outcomes, setOutcomes] = useState<ModUpdateOutcome[]>([]);
  const [runOrder, setRunOrder] = useState<string[]>([]);
  const [runIndex, setRunIndex] = useState(0);
  const [cancelPending, setCancelPending] = useState(false);
  // The mod whose detail modal is open over the wizard, if any.
  const [detailsModId, setDetailsModId] = useState<string | null>(null);

  // The outcomes so far, as the run reads them: it decides from what it has
  // recorded within the same tick it records it, which a re-render would be too
  // late for.
  const outcomesRef = useRef<ModUpdateOutcome[]>([]);
  const cancelRequestedRef = useRef(false);
  // A run outlives many renders, so it calls the latest callback rather than the
  // one from the render it started in.
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
    if (next === 'select') {
      // A run that recorded nothing has nothing to report, so whichever kind it
      // was, it leaves the dialog on its list rather than on an empty account.
      setRunScope('batch');
    }
    setPhase(next);
  }, []);

  const { installMod } = useInstallMod();
  const { cancelInstallMod } = useCancelInstallMod();

  // The mods of the run, one install at a time: each is posted when the one
  // before it has answered, which is what keeps a single compile going and what
  // the outcome list streams.
  const runUpdates = useCallback(
    async (order: string[], sources: Record<string, string>) => {
      for (const [index, modId] of order.entries()) {
        setRunIndex(index);

        const result = await installMod({
          modId,
          modSource: sources[modId],
        });
        if (result.status !== 'reply') {
          // The wizard went away with the install open, taking the run and
          // everything it would have reported with it.
          return;
        }
        const { installedModDetails, uiMissing, error } = result.data;

        if (uiMissing) {
          // A local compile with the development tools absent: this install did not
          // run, and none of the remaining ones would either. The layer has already
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
          setUpdatedModIds((prev) => new Set(prev).add(modId));
          onModUpdatedRef.current(modId, installedModDetails, !error);
        } else if (cancelRequestedRef.current) {
          // A cancelled install still replies, with null details.
          recordOutcome(modId, 'aborted');
        } else {
          // Null details is all a failed install carries - neither host attaches a
          // reason, having sent it to the compiler output window - so the row says
          // that it failed and the run moves on.
          recordOutcome(modId, 'failed');
        }
        if (cancelRequestedRef.current) {
          finish('aborted');
          return;
        }
      }
      finish('done');
    },
    [installMod, finish, recordOutcome]
  );

  // The selected mods that have something to install, each with the source to
  // install, in the order they are listed. The run's order and its sources are
  // both taken from this one pass, so it cannot name a mod it has no source for.
  // A source landing is what makes a row `ready`, so asking for both drops
  // nothing the status alone would keep; it is the two not being able to
  // disagree that is worth having.
  const selectedReady = mods.flatMap((mod) => {
    const source = sources[mod.modId];
    return selected.has(mod.modId) &&
      !updatedModIds.has(mod.modId) &&
      source?.status === 'ready' &&
      source.source
      ? [{ modId: mod.modId, source: source.source }]
      : [];
  });
  // A failed row cannot be updated and is excluded from the count, as is one
  // already updated; a row still loading counts, and is what the Update button
  // waits for.
  const selectedCount = mods.filter(
    (mod) =>
      selected.has(mod.modId) &&
      !updatedModIds.has(mod.modId) &&
      sources[mod.modId]?.status !== 'failed'
  ).length;
  const updateDisabled =
    selectedCount === 0 || selectedReady.length !== selectedCount;

  // Start the run over the mods given, in the order given.
  const startRun = (
    entries: { modId: string; source: string }[],
    scope: 'batch' | 'single'
  ) => {
    // The sources are copied here rather than read as the run goes: the run
    // installs what the user reviewed, whatever a later fetch reports.
    const runSources: Record<string, string> = {};
    for (const { modId, source } of entries) {
      runSources[modId] = source;
    }
    const order = entries.map((entry) => entry.modId);

    outcomesRef.current = [];
    cancelRequestedRef.current = false;

    setOutcomes([]);
    setRunOrder(order);
    setCancelPending(false);
    setRunScope(scope);
    setPhase('running');
    void runUpdates(order, runSources);
  };

  const handleUpdate = () => {
    if (updateDisabled) {
      return;
    }
    startRun(selectedReady, 'batch');
  };

  // One mod updated from its detail modal, which is the same run over a list of
  // one: the install, the cancel and the write-back are the ones every other
  // update gets. What differs is where it is reported - over the list rather than
  // in place of it, so the next mod can be taken up from where this one left off.
  //
  // That modal dismisses itself on the way out and stays clickable through the
  // animation, so the phase is what refuses a second press: it is off the
  // selection by then, and a second run would post an install over the one the
  // first is waiting on.
  const handleUpdateOne = (modId: string) => {
    const source = sources[modId];
    if (
      phase !== 'select' ||
      updatedModIds.has(modId) ||
      source?.status !== 'ready' ||
      !source.source
    ) {
      return;
    }
    startRun([{ modId, source: source.source }], 'single');
  };

  // Put a finished single-mod run away, leaving the list as clean as the run
  // found it: the next mod's run is the first one all over again.
  const dismissSingleRun = () => {
    outcomesRef.current = [];
    setOutcomes([]);
    setRunOrder([]);
    setRunIndex(0);
    setRunScope('batch');
    setPhase('select');
  };

  const handleCancel = async () => {
    // Set whichever way the ack goes, so the run stops after the mod in flight
    // even when the host had nothing to signal - which can mean the cancel got
    // there before the install was registered, leaving it running.
    cancelRequestedRef.current = true;
    setCancelPending(true);

    const modId = runOrder[runIndex];
    if (!modId) {
      // Nothing in flight to name, so there is nothing to ask the host for.
      setCancelPending(false);
      return;
    }

    // The host's reply is not the only end this ask can meet: the install it
    // names can reach its own first, and the wizard can go away with the cancel
    // still on the wire. Neither is answered here - the run ending is what puts
    // the button back up, and a wizard that is gone has no button - so only an
    // ack that found no install to signal is.
    const result = await cancelInstallMod({ modId });
    if (result.status === 'reply' && !result.data.succeeded) {
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
              .filter(
                (modId) =>
                  !updatedModIds.has(modId) &&
                  sources[modId]?.status !== 'failed'
              )
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
  // A single mod's run reports in a modal over the list, which stays where it is:
  // reading one mod up and updating it is a decision the next mod asks again, and
  // a summary in place of the list would end the sitting after the first answer.
  // Its phases drive that modal; the dialog itself stays on its selection.
  const singleRun = runScope === 'single';
  const singleRunOpen = singleRun && phase !== 'select';
  const listPhase: Phase = singleRun ? 'select' : phase;
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
    listPhase === 'select'
      ? {
          display: 'flex',
          flexDirection: 'column',
          minHeight: SELECT_BODY_MIN_HEIGHT,
          maxHeight: maxBodyHeight,
          overflow: 'hidden',
        }
      : listPhase === 'running'
        ? {
            display: 'flex',
            flexDirection: 'column',
            maxHeight: maxBodyHeight,
            overflow: 'hidden',
          }
        : { maxHeight: maxBodyHeight, overflow: 'auto' };

  const footer = (() => {
    switch (listPhase) {
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
        // A single-mod run leaves this footer up under its own modal. Neither
        // button is live while it is: leaving would abandon the install's reply
        // and the write-back that goes with it, and a batch would post a second
        // install over the one in flight.
        return [
          <Button
            key="cancel"
            disabled={singleRunOpen}
            data-testid="mod-updates-cancel"
            onClick={close}
          >
            {t('general.actions.cancel')}
          </Button>,
          <Button
            key="update"
            type="primary"
            disabled={updateDisabled || singleRunOpen}
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
  // The mod a single run is of, which is the only mod in its order.
  const runMod = singleRun
    ? mods.find((mod) => mod.modId === runOrder[0])
    : undefined;

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
        {listPhase === 'select' && (
          <Body>
            <ModUpdateList
              mods={mods}
              sources={sources}
              updatedModIds={updatedModIds}
              selected={selected}
              onToggle={handleToggle}
              onToggleAll={handleToggleAll}
              onRetry={retry}
              onOpenDetails={setDetailsModId}
            />
          </Body>
        )}

        {listPhase === 'running' && (
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

        {listPhase === 'done' && (
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

        {listPhase === 'aborted' && (
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

        {listPhase === 'failed' && (
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

      {singleRunOpen && runMod && (
        <ModUpdateRunModal
          mod={runMod}
          version={sources[runMod.modId]?.version}
          // One row or none: a run that recorded nothing ends back on the
          // selection, so a run still up and past its install has its answer.
          status={outcomes[0]?.status ?? null}
          cancelPending={cancelPending}
          onCancel={handleCancel}
          onClose={dismissSingleRun}
        />
      )}

      {detailsMod && (
        <ModUpdateDetailsModal
          mod={detailsMod}
          source={sources[detailsMod.modId]}
          onLoadInstalledSource={loadInstalledSource}
          onRetrySource={retry}
          onUpdate={() => handleUpdateOne(detailsMod.modId)}
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
                from && to ? <VersionChange from={from} to={to} /> : undefined
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
