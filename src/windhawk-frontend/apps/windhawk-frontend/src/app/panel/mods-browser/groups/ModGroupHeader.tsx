import EllipsisText from '@app/components/EllipsisText';
import { DropdownModal } from '@app/components/InputWithContextMenu';
import { foldingClickHandler } from '@app/panel/shared/foldingClick';
import ModSelectBox, {
  modSelectBoxRevealed,
} from '@app/panel/shared/ModSelectBox';
import { faCaretDown, faChevronDown } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Button, Checkbox, ConfigProvider } from 'antd';
import { useContext } from 'react';
import { useTranslation } from 'react-i18next';
import styled, { css } from 'styled-components';
import { type ModGroup } from './modGroups';

// The glyph's box. The same 12px the settings form's carets are drawn at, so a
// fold reads the same wherever the app offers one.
const CARET_WIDTH = 12;

// The line a block begins with. Not sticky: the selection bar above it is, and a
// second sticky layer under a sticky bar is a stacking order to maintain for a
// header that is already where the block begins.
const Header = styled.div`
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 12px;
  padding-top: 6px;
  padding-bottom: 6px;
  border-bottom: 1px solid var(--whui-border);

  // A block whose every listed mod is selected reads like anything else that is:
  // the same tint, from the same variable. A background color rather than the
  // card's gradient, which is there to tint a background the card already has -
  // the header has none, so the page shows through the translucent color and is
  // tinted by it. Not drawn for the indeterminate state: a partial selection is
  // what the box's own third state says, and tinting the line for it would say
  // the group is selected when it is not.
  &[data-selected] {
    background-color: var(--whui-selected-bg);
  }

  // The box carries the room it keeps from the name, for the lines it stands in
  // that have none of their own. This one is a flex line with a gap, which is
  // that room already, so the box's own would be it twice over.
  ${ModSelectBox} {
    margin-inline-end: 0;
  }

  // What brings the box out: a mod's own predicate, on a line that has no mod
  // on it. A block that is only partly selected is covered by the last rule -
  // something being selected is what the indeterminate state means.
  &:hover ${ModSelectBox},
  &[data-selected] ${ModSelectBox},
  [data-selection-active] & ${ModSelectBox} {
    ${modSelectBoxRevealed}
  }

  @media (hover: none) {
    ${ModSelectBox} {
      ${modSelectBoxRevealed}
    }
  }
`;

// One glyph turned rather than two swapped, so the caret moves between the two
// states instead of jumping. It turns the way the text runs, so a folded block
// points the way its mods would open out.
const CollapseButton = styled(Button)<{ $collapsed: boolean; $rtl: boolean }>`
  flex: none;
  min-width: 0;
  width: ${CARET_WIDTH}px;
  height: auto;
  padding: 0;
  color: var(--whui-text-secondary);

  svg {
    transition: transform 120ms ease-out;

    ${({ $collapsed, $rtl }) =>
      $collapsed &&
      css`
        transform: rotate(${$rtl ? '90deg' : '-90deg'});
      `}
  }
`;

// The name and the count, which are one target: a press on either folds the
// block, and the two read as one phrase either way.
const FoldTarget = styled.div`
  display: flex;
  align-items: baseline;
  gap: 8px;
  min-width: 0;
  cursor: pointer;
  user-select: none;
`;

const GroupName = styled(EllipsisText)`
  font-weight: 500;
`;

const GroupCount = styled.span`
  flex: none;
  color: var(--whui-text-secondary);
`;

const MenuButton = styled(Button)`
  flex: none;
  margin-inline-start: auto;
  padding: 0 6px;
  height: 22px;
`;

/** How much of what the block lists is selected. */
export type ModGroupSelection = 'none' | 'some' | 'all';

interface Props {
  group: ModGroup;
  // How many mods the block lists, which while a filter is active is how many
  // matched rather than how many the group holds.
  modCount: number;
  // Whether the block is drawn folded, which is not the same as what is stored:
  // a filter draws every group open.
  collapsed: boolean;
  selection: ModGroupSelection;
  // False at the ends of the list on screen, where the entry is drawn disabled.
  canMoveUp: boolean;
  canMoveDown: boolean;
  onToggleCollapsed: () => void;
  onSelectionChange: (selected: boolean) => void;
  onMove: (delta: -1 | 1) => void;
  onRename: () => void;
  onDelete: () => void;
}

/**
 * The line a group's block begins with: the caret, the select box, the name, the
 * count, and the menu holding what a group can be asked.
 *
 * The caret and the menu are both always drawn. The caret is the only thing that
 * says a folded group can be opened, and what the menu holds is the only path a
 * keyboard or a screen reader has to reordering, renaming or deleting one. The
 * box is the one control on the line that is revealed rather than drawn, on the
 * same predicate a mod's own box is.
 */
export function ModGroupHeader({
  group,
  modCount,
  collapsed,
  selection,
  canMoveUp,
  canMoveDown,
  onToggleCollapsed,
  onSelectionChange,
  onMove,
  onRename,
  onDelete,
}: Props) {
  const { t } = useTranslation();
  const { direction } = useContext(ConfigProvider.ConfigContext);

  return (
    <Header
      data-testid="mod-group-header"
      data-group-id={group.id}
      data-selected={selection === 'all' ? '' : undefined}
    >
      <CollapseButton
        type="link"
        size="small"
        $collapsed={collapsed}
        $rtl={direction === 'rtl'}
        aria-expanded={!collapsed}
        aria-label={
          t(collapsed ? 'modGroups.expandGroup' : 'modGroups.collapseGroup', {
            name: group.name,
          }) as string
        }
        data-testid="mod-group-caret"
        onClick={onToggleCollapsed}
      >
        <FontAwesomeIcon icon={faChevronDown} />
      </CollapseButton>
      {/* Immediately before the name, which is where a mod's own box stands
          relative to a mod's name. It does not come out from under an edge the
          way a card's does: a header has no border and begins where the block
          does, so there is nothing to travel out from and the box opens where
          it is, moving the name over as a card's does. The caret does not move,
          which is what keeps the line from reading as though the whole header
          had shifted. */}
      <ModSelectBox>
        <Checkbox
          data-testid="mod-group-select"
          aria-label={t('modGroups.selectGroup', { name: group.name }) as string}
          checked={selection === 'all'}
          indeterminate={selection === 'some'}
          // A block that lists nothing has nothing to select, and a box that
          // looks pressable and changes nothing says otherwise.
          disabled={modCount === 0}
          onChange={(e) => onSelectionChange(e.target.checked)}
        />
      </ModSelectBox>
      <FoldTarget
        data-testid="mod-group-name"
        onClick={foldingClickHandler(onToggleCollapsed)}
      >
        <GroupName tooltipPlacement="bottom">{group.name}</GroupName>
        <GroupCount data-testid="mod-group-count">
          {t('modGroups.modCount', { count: modCount })}
        </GroupCount>
      </FoldTarget>
      <DropdownModal
        trigger={['click']}
        menu={{
          items: [
            {
              label: t('modGroups.moveUp'),
              key: 'moveUp',
              disabled: !canMoveUp,
              onClick: () => onMove(-1),
            },
            {
              label: t('modGroups.moveDown'),
              key: 'moveDown',
              disabled: !canMoveDown,
              onClick: () => onMove(1),
            },
            {
              label: t('modGroups.rename'),
              key: 'rename',
              onClick: onRename,
            },
            { type: 'divider', key: 'divider' },
            {
              label: (
                <span data-testid="mod-group-delete">
                  {t('modGroups.delete')}
                </span>
              ),
              key: 'delete',
              danger: true,
              onClick: onDelete,
            },
          ],
        }}
      >
        <MenuButton
          aria-label={t('modGroups.groupActions', { name: group.name }) as string}
          data-testid="mod-group-menu"
        >
          <FontAwesomeIcon icon={faCaretDown} />
        </MenuButton>
      </DropdownModal>
    </Header>
  );
}

export default ModGroupHeader;
