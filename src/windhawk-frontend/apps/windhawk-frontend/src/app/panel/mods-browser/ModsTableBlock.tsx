import EllipsisText from '@app/components/EllipsisText';
import { DropdownModal } from '@app/components/InputWithContextMenu';
import { editMod, forkMod } from '@app/webviewIPC';
import { type ModConfig, type ModMetadata } from '@app/webviewIPCMessages';
import { faCaretDown } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Badge, Button, Checkbox, Modal, Switch, Table, Tag, Tooltip } from 'antd';
import { type ItemType } from 'antd/lib/menu/hooks/useItems';
import { type ColumnsType } from 'antd/lib/table';
import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import styled, { css } from 'styled-components';
import LocalModIcon from '../shared/LocalModIcon';
import ModSelectBox, {
  modSelectBoxReach,
  modSelectBoxRevealed,
  modSelectBoxRoom,
} from '../shared/ModSelectBox';

// The room between the cell's border and the line its name is on: antd's cell
// padding at size="middle". It is what the checkbox travels across.
const TABLE_CELL_PADDING = 8;

// The name cell: the checkbox's room, and then everything the cell says about
// the mod. The two are separated so the box can move the line over without
// narrowing it - the items keep the cell's whole width whatever the box is
// doing, so no wrap is ever re-decided and the row is exactly as tall as it was.
// What the box pushes past the cell's edge is clipped there. Taking the room out
// of the line instead would have a name beside its update tag gain a second line
// the moment the pointer arrived, and carry every row below it down.
//
// The line reaches back over the cell's padding to its border, which is both
// where the checkbox comes out from and where anything the shift pushes off the
// other end is cut.
const ModNameCellContent = styled.span`
  display: flex;
  align-items: center;

  ${modSelectBoxReach(TABLE_CELL_PADDING)}
`;

const ModNameCellItems = styled.span`
  display: flex;
  flex: none;
  width: 100%;
  gap: 8px;
  flex-wrap: wrap;
  align-items: center;
`;

const ModNameLink = styled.a`
  color: var(--whui-link);

  &:hover {
    color: var(--whui-link-hover);
  }
`;

// The line the box moved over runs the room it took past the cell's edge, and
// goes out under a fade rather than against a cut. The fade is exactly that
// room, so it covers what can be lost and touches nothing on a line with space
// to spare - which is most of them.
const nameCellFade = css`
  mask-image: linear-gradient(
    to var(--mods-table-clip-edge),
    #000 calc(100% - ${modSelectBoxRoom}px),
    transparent
  );
`;

// The rows whose checkbox is out. Spelled once: the box and the line it moves
// both answer to it.
//
// The last one reads the attribute off an ancestor of the whole table - the
// blocks container, which is above every block and therefore above the group
// headers too. Hence `&` in the middle rather than the front: written the other
// way round it would compile to `.wrapper [data-selection-active] ...` and ask
// for the attribute inside the table, where it no longer is.
const rowsWithTheBoxOut = `
  .ant-table-tbody > tr:hover,
  .ant-table-tbody > tr:focus-within,
  .ant-table-tbody > tr[data-selected],
  [data-selection-active] & .ant-table-tbody > tr
`;

// antd's per-cell border-right (the column separators) is dropped in production
// by a cssnano :is()-folding bug (https://github.com/cssnano/cssnano/issues/1786,
// fixed in cssnano 8.0.1; this build ships 7.1.x); redeclaring via
// styled-components bypasses the minifier. The right edge sits on the container
// rather than on the last cell, whose border Chromium drops under
// table-layout: fixed; clearing that cell's border keeps the edge a single line
// where antd's rule does survive the minifier.
// --whui-border matches antd's table border color per theme.
const ModsTable = styled(Table)`
  .ant-table.ant-table-bordered .ant-table-cell:not(:last-child) {
    border-right: 1px solid var(--whui-border);
  }

  .ant-table.ant-table-bordered .ant-table-cell:last-child {
    border-right: 0;
  }

  .ant-table.ant-table-bordered > .ant-table-container {
    border-right: 1px solid var(--whui-border);
  }

  // Which edge the name cell's line runs off, which the fade below has to sit
  // on. A mask has no logical direction of its own, and antd flips the table for
  // RTL, so this is the one thing here that needs to be told which way is out.
  --mods-table-clip-edge: right;

  &.ant-table-wrapper-rtl {
    --mods-table-clip-edge: left;
  }

  // What brings a row's checkbox out, on the card's predicate: the row's own
  // hover, focus anywhere in it, the mod being checked, and - read past the
  // table, off the blocks above it - anything at all being checked. Per row
  // rather than per table body, which is the nicer idea and is not available
  // here: it would shift every name in the table the moment the pointer crossed
  // into it. A selection under way does bring them all out, as it does the whole
  // grid, so the names move on the way into a selection and on the way out of
  // one and the table is stable in between.
  ${rowsWithTheBoxOut} {
    ${ModSelectBox} {
      ${modSelectBoxRevealed}
    }

    ${ModNameCellContent} {
      ${nameCellFade}
    }
  }

  // A device with no pointer has nothing to reveal them with, so there they
  // simply stand. Asked as whether hover exists rather than whether the device
  // is a phone: a touch laptop has both a finger and a pointer.
  @media (hover: none) {
    ${ModSelectBox} {
      ${modSelectBoxRevealed}
    }

    ${ModNameCellContent} {
      ${nameCellFade}
    }
  }

  // Once the pointer moves on, a 16px checkbox is the only mark a selected mod
  // carries, which is too thin to confirm a removal of eight against. The row
  // takes the card's tint, from the same variable off the same attribute, so
  // "selected" is one color in the app rather than two that happen to mean the
  // same thing. A gradient rather than a background color: the tint is
  // translucent, and a translucent background color would replace the cell's own
  // - antd's hover shade among them - instead of tinting it.
  .ant-table-tbody > tr[data-selected] > td {
    background-image: linear-gradient(
      var(--whui-selected-bg),
      var(--whui-selected-bg)
    );
  }
` as typeof Table;

const TableActionsButton = styled(Button)`
  padding: 0 6px;
  height: 22px;
`;

const ModLocalIcon = styled(LocalModIcon)`
  width: 20px;
  height: 20px;
`;

// The mod a row is drawn from, for the parts of it no column of the row states:
// whether an update is on offer is `updateAvailable` below, reached by the
// screen that holds the mod.
type ModDetailsType = {
  metadata: ModMetadata | null;
  config: ModConfig | null;
  userRating: number;
};

export type ModTableRow = {
  key: string;
  modId: string;
  name: string;
  description?: string;
  author?: string;
  version?: string;
  isLocal: boolean;
  updateAvailable: boolean;
  disabled: boolean;
  notCompiled: boolean;
  selected: boolean;
  mod: ModDetailsType;
};

// A row as the list is built, before the selection has been read onto it: the
// selection is what a sort must not depend on, since the sorted order is what
// the selection ranges over.
export type ModTableRowData = Omit<ModTableRow, 'selected'>;

export type ModTableSortKey = 'name' | 'author' | 'version' | 'status';

export type ModTableSort = {
  key: ModTableSortKey;
  order: 'ascend' | 'descend';
};

// How each sortable column orders the rows. Named here rather than written into
// the column definitions so that the order a table shows is an order its caller
// can also hand to the selection - a shift-click has to fill the run the user
// sees, and a sorted table is not the order the filter left.
export const modTableSorters: Record<
  ModTableSortKey,
  (a: ModTableRowData, b: ModTableRowData) => number
> = {
  name: (a, b) => a.name.localeCompare(b.name),
  author: (a, b) => (a.author || '').localeCompare(b.author || ''),
  version: (a, b) =>
    (a.version || '').localeCompare(b.version || '', undefined, {
      numeric: true,
      sensitivity: 'base',
    }),
  status: (a, b) => Number(a.disabled) - Number(b.disabled),
};

/** What a row's controls do, gathered so the columns can close over one value. */
export type ModsTableActions = {
  devModeOptOut?: boolean;
  toggleSelected: (modId: string, selected: boolean, shiftKey: boolean) => void;
  enableMod: (modId: string, enable: boolean) => void;
  compileMod: (modId: string) => void;
  deleteMod: (modId: string) => void;
  openModDetails: (modId: string) => void;
  setConfirmModalOpen: (open: boolean) => void;
};

interface Props {
  rows: ModTableRow[];
  // Which column this table is sorted by. Held by the caller rather than here,
  // and rows arrive in the order it says: the order on screen is the order a
  // shift-click ranges over, which is a thing only the screen holding every
  // block can put together.
  sort: ModTableSort | null;
  onSortChange: (sort: ModTableSort | null) => void;
  actions: ModsTableActions;
}

/**
 * One list of mods as a table, with the column sort of its own that makes it a
 * separate list rather than a section of one.
 */
export function ModsTableBlock({ rows, sort, onSortChange, actions }: Props) {
  const { t } = useTranslation();

  const {
    devModeOptOut,
    toggleSelected,
    enableMod,
    compileMod,
    deleteMod,
    openModDetails,
    setConfirmModalOpen,
  } = actions;

  const columns = useMemo<ColumnsType<ModTableRow>>(
    () => [
      {
        title: '',
        key: 'actions',
        width: 50,
        align: 'center',
        render: (_, record) => {
          const isLocal = record.isLocal;
          const menuItems: ItemType[] = [];

          // Compile action (if not compiled)
          if (record.notCompiled) {
            menuItems.push({
              label: t('mod.compile'),
              key: 'compile',
              onClick: () => {
                compileMod(record.modId);
              },
            });
          }

          // Enable/Disable action (if compiled)
          if (!record.notCompiled) {
            menuItems.push({
              label: record.disabled
                ? t('mod.enable')
                : t('mod.disable'),
              key: 'toggle-enable',
              onClick: () => {
                enableMod(record.modId, record.disabled);
              },
            });
          }

          // Dev actions (only when development mode is enabled)
          if (!devModeOptOut) {
            // Divider before dev actions
            if (menuItems.length > 0) {
              menuItems.push({ type: 'divider' });
            }

            // Edit action (local mods only)
            if (isLocal) {
              menuItems.push({
                label: t('mod.edit'),
                key: 'edit',
                onClick: () => {
                  editMod({ modId: record.modId });
                },
              });
            }

            // Fork action
            menuItems.push({
              label: t('mod.fork'),
              key: 'fork',
              onClick: () => {
                forkMod({ modId: record.modId });
              },
            });
          }

          // Divider before remove
          menuItems.push({ type: 'divider' });

          // Remove action
          menuItems.push({
            label: t('mod.remove'),
            key: 'remove',
            danger: true,
            onClick: () => {
              setConfirmModalOpen(true);
              Modal.confirm({
                title: t('mod.removeConfirm'),
                okText: t('mod.removeConfirmOk'),
                cancelText: t('general.actions.cancel'),
                okButtonProps: { danger: true },
                onOk: () => {
                  setConfirmModalOpen(false);
                  deleteMod(record.modId);
                },
                onCancel: () => {
                  setConfirmModalOpen(false);
                },
                closable: true,
                maskClosable: true,
              });
            },
          });

          const hasLogging = record.mod.config?.loggingEnabled || record.mod.config?.debugLoggingEnabled;
          const actionsButton = (
            <DropdownModal
              menu={{ items: menuItems }}
              trigger={['click']}
            >
              <TableActionsButton>
                <FontAwesomeIcon icon={faCaretDown} />
              </TableActionsButton>
            </DropdownModal>
          );

          if (hasLogging) {
            return (
              <Badge
                dot
                title={t('mod.loggingEnabledInAdvancedTab') as string}
                status="warning"
              >
                {actionsButton}
              </Badge>
            );
          }

          return actionsButton;
        },
      },
      {
        title: t('home.installedMods.grid.name'),
        dataIndex: 'name',
        key: 'name',
        width: '30%',
        sorter: modTableSorters.name,
        sortOrder: sort?.key === 'name' ? sort.order : null,
        render: (name, record) => (
          <ModNameCellContent>
            {/* At the head of the line the mod's name is on, which is where
                the card's is: the same control in the same place relative to
                the name, so the two views read as one feature. The name is a
                link here, so the room the box takes moves a click target
                rather than just text - the accepted cost of not putting the
                two views' checkboxes in different places. */}
            <ModSelectBox>
              <Checkbox
                data-testid="mod-row-select"
                aria-label={t('modSelection.selectMod', { name }) as string}
                checked={record.selected}
                onChange={(e) =>
                  toggleSelected(
                    record.modId,
                    e.target.checked,
                    e.nativeEvent.shiftKey
                  )
                }
              />
            </ModSelectBox>
            <ModNameCellItems>
              <ModNameLink onClick={() => openModDetails(record.modId)}>
                {name}
              </ModNameLink>
              {record.updateAvailable && (
                <Tag color="warning" style={{ margin: 0, userSelect: 'none' }}>
                  {t('mod.updateAvailable')}
                </Tag>
              )}
              {record.isLocal && (
                <Tooltip title={t('mod.editedLocally')} placement="bottom">
                  <ModLocalIcon aria-label={t('mod.editedLocally')} />
                </Tooltip>
              )}
            </ModNameCellItems>
          </ModNameCellContent>
        ),
      },
      {
        title: t('home.installedMods.grid.description'),
        dataIndex: 'description',
        key: 'description',
        render: (description) => (
          <EllipsisText tooltipPlacement="bottom">{description || '-'}</EllipsisText>
        ),
        ellipsis: { showTitle: false },
      },
      {
        title: t('home.installedMods.grid.author'),
        dataIndex: 'author',
        key: 'author',
        width: '12%',
        sorter: modTableSorters.author,
        sortOrder: sort?.key === 'author' ? sort.order : null,
        render: (author) => author || '-',
      },
      {
        title: t('home.installedMods.grid.version'),
        dataIndex: 'version',
        key: 'version',
        width: '8%',
        sorter: modTableSorters.version,
        sortOrder: sort?.key === 'version' ? sort.order : null,
        render: (version) => version || '-',
      },
      {
        title: t('home.installedMods.grid.status'),
        key: 'status',
        width: 80,
        align: 'center',
        sorter: modTableSorters.status,
        sortOrder: sort?.key === 'status' ? sort.order : null,
        render: (_, record) => (
          <Switch
            checked={!record.disabled}
            disabled={record.notCompiled}
            onChange={(checked) => enableMod(record.modId, checked)}
            title={
              record.notCompiled
                ? (t('mod.notCompiled') as string)
                : undefined
            }
          />
        ),
      },
    ],
    [
      t,
      devModeOptOut,
      compileMod,
      enableMod,
      deleteMod,
      openModDetails,
      setConfirmModalOpen,
      toggleSelected,
      sort,
    ]
  );

  return (
    <ModsTable
      bordered
      data-testid="installed-mods-table"
      dataSource={rows}
      columns={columns}
      // The header says what to sort by; the rows are ordered by the caller from
      // what it says, so that the order on screen is one the selection can range
      // over too.
      onChange={(pagination, filters, sorter) => {
        const sorted = Array.isArray(sorter) ? sorter[0] : sorter;
        const key = sorted?.columnKey as ModTableSortKey | undefined;
        onSortChange(
          key && sorted?.order && key in modTableSorters
            ? { key, order: sorted.order }
            : null
        );
      }}
      // The mark the row's tint is drawn from, and the one the card carries, so
      // one selector shape serves both views. Cast because React's attribute
      // types describe data-* in JSX only, and these are handed over as a plain
      // object.
      onRow={(record: ModTableRow) =>
        ({
          'data-selected': record.selected ? '' : undefined,
        }) as React.HTMLAttributes<HTMLTableRowElement>
      }
      pagination={false}
      size="middle"
      showSorterTooltip={false}
      style={{ wordBreak: 'break-word' }}
    />
  );
}

export default ModsTableBlock;
