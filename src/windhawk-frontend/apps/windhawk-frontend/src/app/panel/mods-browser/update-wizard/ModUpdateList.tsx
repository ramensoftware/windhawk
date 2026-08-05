import { Button, Checkbox, Spin } from 'antd';
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
const Grid = styled.div`
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

const HeaderCell = styled.div`
  ${cellLayout}
  position: sticky;
  top: 0;
  z-index: 1;
  font-weight: 600;
  background: var(--whui-card-background-color);
`;

// The line above a row's cells is what separates it from the row before, and the
// first row's is what separates the list from its headings.
const Cell = styled.div`
  ${cellLayout}
  border-top: 1px solid var(--whui-divider);
`;

// Contributes no box of its own, so its cells are the grid's children and land in
// the shared columns. It still carries the row's identity for the tests.
const Row = styled.div`
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
 * dropping out of a list the user is counting.
 */
export function ModUpdateList({
  mods,
  sources,
  selected,
  onToggle,
  onToggleAll,
  onRetry,
  onOpenDetails,
}: Props) {
  const { t } = useTranslation();

  const selectable = mods.filter(
    (mod) => sources[mod.modId]?.status !== 'failed'
  );
  const selectedCount = selectable.filter((mod) =>
    selected.has(mod.modId)
  ).length;

  return (
    <Grid>
      <HeaderCell>
        <Checkbox
          data-testid="mod-update-select-all"
          disabled={selectable.length === 0}
          checked={selectable.length > 0 && selectedCount === selectable.length}
          indeterminate={selectedCount > 0 && selectedCount < selectable.length}
          onChange={(e) => onToggleAll(e.target.checked)}
          aria-label={t('modUpdates.selectAll') as string}
        />
      </HeaderCell>
      <HeaderCell>{t('modUpdates.columns.mod')}</HeaderCell>
      <HeaderCell>{t('modUpdates.columns.installed')}</HeaderCell>
      <HeaderCell>{t('modUpdates.columns.new')}</HeaderCell>
      <HeaderCell />
      {mods.map((mod) => {
        const source = sources[mod.modId];
        const failed = source?.status === 'failed';
        const ready = source?.status === 'ready';
        return (
          <Row
            key={mod.modId}
            data-testid="mod-update-row"
            data-mod-id={mod.modId}
            data-status={source?.status ?? 'loading'}
          >
            <Cell>
              <Checkbox
                data-testid="mod-update-include"
                checked={!failed && selected.has(mod.modId)}
                disabled={failed}
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
            <Cell>
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
                {failed ? (
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
