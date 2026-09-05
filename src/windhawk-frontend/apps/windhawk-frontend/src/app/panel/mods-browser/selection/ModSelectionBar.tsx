import { Button, Tooltip } from 'antd';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { type ActionTargets } from './selectionActions';

// How far the bar's corners are rounded: antd's @border-radius-base, which is
// what everything it stands among is cut to - the cards and the table below it,
// the search row's input and buttons above it, and the updates alert above
// those. Named because the page behind the bar is cut to the same curve along
// its bottom edge, and the two only meet on the pixel for as long as they are
// the same number.
const BAR_RADIUS = 2;

// A plain container rather than an antd Alert, which renders role="alert": that
// is right for an announcement made once and wrong for a strip of controls that
// stays while the user works. It is not role="toolbar" either - that role
// promises arrow-key navigation between the buttons, which this does not
// implement, and tab order already reaches them.
const Bar = styled.div`
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
  padding-block: 8px;
  padding-inline: 12px;
  border: 1px solid var(--whui-border);
  border-radius: ${BAR_RADIUS}px;
  background-color: var(--whui-float-bg);
`;

// What holds the bar at the top of the scroll region, so that a selection made
// at the head of a long grid can still be acted on at the foot of it. It
// releases at the end of the installed section, which is the container the
// caller wraps it and the list in.
//
// The z-index is what keeps it over the list it rides on. Nothing there is
// content, but plenty of it is drawn from a positioned element - a card's
// update ribbon, and every layer antd's table stacks, up to the 4 its fixed
// columns cast their shadow from. The one that decides the number is
// .ant-table-column-title, at 1: at equal z-index the later element in the tree
// wins, which is the header of the list the bar is standing over.
//
// It carries the page color for the sake of the bar's top corners: a rounded box
// leaves the square each corner is cut from unpainted, and with the list running
// underneath, that square is whatever card or cell border happens to be passing.
// This is a wrapper rather than anything inside the bar because the page has to
// go *behind* the bar's own surface, and nothing in it can - an element paints
// its background before its children, negative z-index and all.
//
// Along the bottom it is cut to the bar's own curve instead, and stops there:
// that edge is the one the list comes out from, and a square of page in each
// corner would read as two notches punched out of the row passing under.
const StickyRegion = styled.div`
  position: sticky;
  top: 0;
  z-index: 5;
  margin-bottom: 20px;
  background-color: var(--whui-background-color);
  border-radius: 0 0 ${BAR_RADIUS}px ${BAR_RADIUS}px;
`;

const Actions = styled.div`
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  margin-inline-start: auto;
`;

interface Props {
  // How many listed mods are checked. Renders nothing at zero, so the caller can
  // hand over the count without guarding the bar itself.
  selectedCount: number;
  targets: ActionTargets;
  allSelected: boolean;
  // Held while any enable or delete this screen posted is unanswered.
  busy: boolean;
  onEnable: () => void;
  onDisable: () => void;
  onMoveToGroup: () => void;
  onRemove: () => void;
  onSelectAll: () => void;
  onClear: () => void;
}

/**
 * The bar above the installed mods that acts on the ones that are checked.
 *
 * Presentational: it says how many are selected and how many each action would
 * reach, and acts on nothing by itself.
 *
 * Nothing on it opens a popup. That matters once the bar is sticky:
 * usePopupDismissOnScroll closes any open popup on any scroll, so an action
 * parked behind a dropdown would be dismissed under a user scrolling to check
 * what they had selected.
 */
export function ModSelectionBar({
  selectedCount,
  targets,
  allSelected,
  busy,
  onEnable,
  onDisable,
  onMoveToGroup,
  onRemove,
  onSelectAll,
  onClear,
}: Props) {
  const { t } = useTranslation();

  if (selectedCount <= 0) {
    return null;
  }

  return (
    <StickyRegion>
      <Bar data-testid="mod-selection-bar">
        {/* Announced as it changes, so checking a mod says what the selection
            now holds rather than leaving a screen reader user to go and find
            it. */}
        <span data-testid="mod-selection-count" aria-live="polite">
          {t('modSelection.selected', { count: selectedCount })}
        </span>
        <Actions>
          {/* Hidden rather than disabled once everything is selected: there is
              nothing left for it to do, and Clear is right beside it. */}
          {!allSelected && (
            <Button
              size="small"
              data-testid="mod-selection-select-all"
              onClick={onSelectAll}
            >
              {t('modSelection.selectAll')}
            </Button>
          )}
          <Button
            size="small"
            data-testid="mod-selection-clear"
            onClick={onClear}
          >
            {t('modSelection.clear')}
          </Button>
          {/* The count of what each action would reach lives in the tooltip rather
              than in the label, so the buttons do not resize as the selection
              changes. */}
          <Tooltip
            title={
              targets.enable.length === 0
                ? t('modSelection.nothingToEnable')
                : t('modSelection.enableCount', { count: targets.enable.length })
            }
            placement="bottom"
          >
            <Button
              size="small"
              disabled={busy || targets.enable.length === 0}
              data-testid="mod-selection-enable"
              onClick={onEnable}
            >
              {t('mod.enable')}
            </Button>
          </Tooltip>
          <Tooltip
            title={
              targets.disable.length === 0
                ? t('modSelection.nothingToDisable')
                : t('modSelection.disableCount', { count: targets.disable.length })
            }
            placement="bottom"
          >
            <Button
              size="small"
              disabled={busy || targets.disable.length === 0}
              data-testid="mod-selection-disable"
              onClick={onDisable}
            >
              {t('mod.disable')}
            </Button>
          </Tooltip>
          {/* The fourth action, and the one that earns the slot: enable, disable
              and remove all have a per-mod control elsewhere on the screen, and
              grouping has none. What it opens is a modal rather than a popup,
              for the same reason nothing else here opens one. */}
          <Button
            size="small"
            data-testid="mod-selection-move"
            onClick={onMoveToGroup}
          >
            {t('modGroups.moveToGroup')}
          </Button>
          {/* Always available: every selected mod can be removed. */}
          <Tooltip
            title={t('modSelection.removeCount', { count: targets.remove.length })}
            placement="bottom"
          >
            <Button
              size="small"
              danger
              disabled={busy}
              data-testid="mod-selection-remove"
              onClick={onRemove}
            >
              {t('mod.remove')}
            </Button>
          </Tooltip>
        </Actions>
      </Bar>
    </StickyRegion>
  );
}

export default ModSelectionBar;
