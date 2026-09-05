import { Button, Checkbox, Spin, Tag } from 'antd';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';

import type { ModUpdateSource } from './useModUpdateSources';

// One installed mod with an update waiting, as the wizard is handed it.
export type UpdatableMod = {
  modId: string;
  name: string;
  installedVersion?: string;
};

// Every cell of the list is a child of this one grid - the rows are
// `display: contents` - so a column is sized once, from all of its cells at
// once. A grid per row would size each row's columns from that row alone, and
// the header's from the headings alone, which is to say they would not line up.
// It is also the scroll region; the headings stay put by sticking to its top.
//
// The roles here and on the cells below are what makes the same thing a table to
// a screen reader: which column a cell is in is all that says whether a version
// is the installed one or the one on offer, and only a table relates a heading to
// the cells under it. Laying a real <table> out as a grid would not have kept its
// semantics either - a display other than the table ones drops them - so the
// roles would be written out whichever element this was.
const Grid = styled.div.attrs({ role: 'table' })`
  display: grid;
  grid-template-columns: auto minmax(0, 1fr) auto auto auto;
  // Rows keep the height their content asks for; the space a short list leaves
  // over is space, not four rows stretched to fill the phase's floor.
  align-content: start;
  flex: 1 1 auto;
  min-height: 0;
  overflow: auto;
  border: 1px solid var(--whui-border);
  border-radius: 6px;
  background: var(--whui-card-background-color);
`;

// Cells stretch to their row's full height (the grid's default alignment) and
// center what is in them, rather than shrinking to their own content and floating
// to the middle of the track. Centering the cells instead would start each one at
// a different height, which is where the separator below is drawn - so a row's
// line would come out as staggered segments rather than one rule.
const cellLayout = `
  display: flex;
  flex-direction: column;
  justify-content: center;
  padding: 8px 12px;
`;

const HeaderCell = styled.div.attrs({ role: 'columnheader' })`
  ${cellLayout}
  position: sticky;
  top: 0;
  z-index: 1;
  font-weight: 600;
  background: var(--whui-card-background-color);
`;

// The line above a row's cells is what separates it from the row before, and the
// first row's is what separates the list from its headings.
const Cell = styled.div.attrs({ role: 'cell' })`
  ${cellLayout}
  border-top: 1px solid var(--whui-divider);
`;

// Contributes no box of its own, so its cells are the grid's children and land in
// the shared columns. It still carries the row's identity for the tests, and the
// role earns it a place in the accessibility tree, which a box-less element is
// otherwise left out of. Without rows to gather them, the cells would sit under
// no heading.
const Row = styled.div.attrs({ role: 'row' })`
  display: contents;
`;

const ModName = styled.div`
  overflow-wrap: break-word;
`;

const Version = styled.div`
  color: var(--whui-text-muted);
  white-space: nowrap;
`;

const NewVersion = styled(Version)`
  color: inherit;
`;

// Sits under the mod's name rather than in the New column, which it would
// otherwise widen for every row.
const FailedNote = styled.div`
  color: var(--whui-text-muted);
  font-size: 13px;
`;

const Actions = styled.div`
  display: flex;
  justify-content: end;
`;

interface Props {
  mods: UpdatableMod[];
  sources: Record<string, ModUpdateSource>;
  // The mods this sitting has already updated, one at a time from their own
  // modals. Their rows are done: there is nothing left to install for them.
  updatedModIds: Set<string>;
  selected: Set<string>;
  onToggle: (modId: string, checked: boolean) => void;
  onToggleAll: (checked: boolean) => void;
  onRetry: (modId: string) => void;
  onOpenDetails: (modId: string) => void;
}

/**
 * The select phase's list: one row per updatable mod, with what it is on, what it
 * would move to, and the way to read up on the difference first.
 *
 * A mod whose source could not be fetched cannot be updated - there is nothing to
 * install - so its row is unselectable and offers a retry instead, rather than
 * dropping out of a list the user is counting. A mod already updated keeps its
 * row for the same reason, saying so where the retry or the changelog would be.
 */
export function ModUpdateList({
  mods,
  sources,
  updatedModIds,
  selected,
  onToggle,
  onToggleAll,
  onRetry,
  onOpenDetails,
}: Props) {
  const { t } = useTranslation();

  const selectable = mods.filter(
    (mod) =>
      !updatedModIds.has(mod.modId) && sources[mod.modId]?.status !== 'failed'
  );
  const selectedCount = selectable.filter((mod) =>
    selected.has(mod.modId)
  ).length;

  return (
    <Grid aria-label={t('modUpdates.title') as string}>
      <Row>
        <HeaderCell>
          <Checkbox
            data-testid="mod-update-select-all"
            disabled={selectable.length === 0}
            checked={
              selectable.length > 0 && selectedCount === selectable.length
            }
            indeterminate={
              selectedCount > 0 && selectedCount < selectable.length
            }
            onChange={(e) => onToggleAll(e.target.checked)}
            aria-label={t('modUpdates.selectAll') as string}
          />
        </HeaderCell>
        <HeaderCell>{t('modUpdates.columns.mod')}</HeaderCell>
        <HeaderCell>{t('modUpdates.columns.installed')}</HeaderCell>
        <HeaderCell>{t('modUpdates.columns.new')}</HeaderCell>
        <HeaderCell />
      </Row>
      {mods.map((mod) => {
        const source = sources[mod.modId];
        const updated = updatedModIds.has(mod.modId);
        const failed = source?.status === 'failed';
        const ready = source?.status === 'ready';
        const loading = !failed && !ready;
        return (
          <Row
            key={mod.modId}
            data-testid="mod-update-row"
            data-mod-id={mod.modId}
            data-status={updated ? 'updated' : (source?.status ?? 'loading')}
          >
            <Cell>
              <Checkbox
                data-testid="mod-update-include"
                checked={!failed && !updated && selected.has(mod.modId)}
                disabled={failed || updated}
                onChange={(e) => onToggle(mod.modId, e.target.checked)}
                aria-label={mod.name}
              />
            </Cell>
            <Cell>
              <ModName>{mod.name}</ModName>
              {failed && <FailedNote>{t('modUpdates.sourceFailed')}</FailedNote>}
            </Cell>
            <Cell>
              <Version>{mod.installedVersion || '-'}</Version>
            </Cell>
            <Cell
              // A spinner reads out as nothing, so while it is up the cell says
              // what it is waiting on. Only while it is up: a name on a cell
              // stands in for what is in it, which the rest of the time is the
              // version to read.
              aria-label={
                loading ? (t('general.status.loading') as string) : undefined
              }
            >
              {failed ? (
                <Version>-</Version>
              ) : ready ? (
                <NewVersion data-testid="mod-update-new-version">
                  {source.version || '-'}
                </NewVersion>
              ) : (
                <Spin size="small" />
              )}
            </Cell>
            <Cell>
              <Actions>
                {updated ? (
                  // The same word the summary of a batch run tags a mod that
                  // landed with, this row being the whole report of a run of one.
                  <Tag color="success" data-testid="mod-update-done">
                    {t('modUpdates.modStatus.updated')}
                  </Tag>
                ) : failed ? (
                  <Button
                    size="small"
                    data-testid="mod-update-retry"
                    onClick={() => onRetry(mod.modId)}
                  >
                    {t('general.status.tryAgain')}
                  </Button>
                ) : (
                  <Button
                    size="small"
                    data-testid="mod-update-changelog"
                    onClick={() => onOpenDetails(mod.modId)}
                  >
                    {t('modDetails.changelog.title')}
                  </Button>
                )}
              </Actions>
            </Cell>
          </Row>
        );
      })}
    </Grid>
  );
}

export default ModUpdateList;
