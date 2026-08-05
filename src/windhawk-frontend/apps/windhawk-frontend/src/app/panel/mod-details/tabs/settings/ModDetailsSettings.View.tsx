import { DropdownModal, InputNumberWithContextMenu, InputWithContextMenu, PopconfirmModal, SelectModal } from '@app/components/InputWithContextMenu';
import useKeyboardShortcut from '@app/panel/shared/useKeyboardShortcut';
import usePersistedFlag from '@app/panel/shared/usePersistedFlag';
import {
  type InitialSettingItem,
  type InitialSettings,
  type InitialSettingsArrayValue,
  type InitialSettingsValue,
} from '@app/webviewIPCMessages';
import {
  faCaretDown,
  faChevronDown,
  faCircleInfo,
  faCompress,
  faExpand,
  faGripVertical,
  faRotateLeft,
  faTableList,
  faTextWidth,
} from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import {
  Alert,
  Button,
  Card,
  ConfigProvider,
  List,
  Segmented,
  Select,
  Switch,
  Tooltip,
} from 'antd';
import {
  createContext,
  lazy,
  Suspense,
  useCallback,
  useContext,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import styled, { createGlobalStyle, css, keyframes } from 'styled-components';
import { type ModSettings, describeSetting, INT32_MAX, INT32_MIN, parseIntLax, SettingType } from './core/yamlConverter';
import {
  indexAtPrefix,
  isKeyUnder,
  isSubtreeChanged,
  materializedMaxIndex,
  rewriteKeyAfterMove,
  rewriteKeyAfterRemove,
} from './core/editorState';
import {
  flattenAllDefaults,
  formatDefaultValue,
  isSettingModified,
} from './core/settingDefaults';
import { type EditorViewModel } from './useModSettingsEditor';

// Use webpack constant for conditional compilation
declare const WEBPACK_IS_WEBSITE: boolean;

// Lazy-load Monaco editor only in extension mode
const MonacoYamlEditor = WEBPACK_IS_WEBSITE
  ? null
  : lazy(() => import('./MonacoYamlEditor'));

// ============================================================================
// Styled Components
// ============================================================================

// How tightly the form is drawn. Compact tightens every gap and takes the
// descriptions off the rows, leaving them a hover away. Purely how the settings
// are shown: nothing about the values or the edits depends on it.
type Density = 'comfortable' | 'compact';

const SETTINGS_DENSITY_STORAGE_KEY = 'settingsCompactMode';
const SETTINGS_WORD_WRAP_STORAGE_KEY = 'settingsWordWrap';

// How far the strip a row's grip is drawn in stands off the row's start edge.
const ARRAY_ITEM_HANDLE_OFFSET = '2px';

// Everything the density decides: the gaps, and the size antd draws a control
// at - a form that tightens its gaps but leaves the inputs at full height only
// trades one kind of slack for another.
const DENSITY = {
  comfortable: {
    controlSize: 'middle',
    formPadding: '12px',
    rowPadding: '12px',
    valueRowPadding: '4px',
    metaMargin: '8px',
    cardPadding: '24px',
    arrayControlHeight: '32px',
    arrayHandleWidth: '20px',
    arrayMenuPadding: '10px',
  },
  compact: {
    controlSize: 'small',
    formPadding: '6px',
    rowPadding: '6px',
    valueRowPadding: '2px',
    metaMargin: '4px',
    cardPadding: '18px',
    arrayControlHeight: '24px',
    arrayHandleWidth: '16px',
    arrayMenuPadding: '6px',
  },
} as const;

// The density's gaps, as custom properties on the form's wrapper. Everything
// inside reads them off the cascade, so no level of the tree carries the density
// down to the next and no styled component takes a prop for it.
//
// The --whui-* namespace is the webview's own: --wh-* belongs to the Tauri host
// and --vscode-* to the extension host, and neither side may clobber the other.
function densityVariables(density: Density) {
  const gaps = DENSITY[density];

  return css`
    --whui-settings-form-padding: ${gaps.formPadding};

    // Set here rather than left to antd's, so the bar marking a row can be
    // inset by the same amount at either density.
    --whui-settings-row-padding: ${gaps.rowPadding};

    // A row of an array of plain values holds one input and nothing else, so
    // the gap only keeps two inputs apart.
    --whui-settings-value-row-padding: ${gaps.valueRowPadding};

    --whui-settings-meta-margin: ${gaps.metaMargin};

    // Also the gutter the rows inside hang what marks them and what acts on
    // them in - the state bar, the fold caret, the grip - so it is held to what
    // the widest of those reaches.
    --whui-settings-card-padding: ${gaps.cardPadding};

    // The height of one line of an array: the controls beside a row, and the
    // line a folded row is left as, which is the height of the input the fold
    // took away.
    --whui-settings-array-control-height: ${gaps.arrayControlHeight};

    // Wider than the icon it holds, so the grip can be hit without being aimed
    // at.
    --whui-settings-array-handle-width: ${gaps.arrayHandleWidth};

    // The whole strip the grip is drawn in, which the row reaches out into -
    // the gap included, so there is no sliver where the pointer is on neither.
    --whui-settings-array-handle-gutter: calc(
      ${gaps.arrayHandleWidth} + ${ARRAY_ITEM_HANDLE_OFFSET}
    );

    --whui-settings-array-menu-padding: ${gaps.arrayMenuPadding};
  `;
}

// The fold caret's box, and the gap the title line leaves between the things
// sharing it. The caret hangs in the gutter rather than standing in the line, so
// a title with a form behind it starts where a title without one does.
const COLLAPSE_CARET_WIDTH = 12;
const SETTING_TITLE_GAP = 8;

// What the caret keeps between itself and the name, on top of what its box
// already leaves: enough to read as beside the name, not as its first letter.
const COLLAPSE_CARET_GAP = 2;

// How far back from the row's content edge the caret reaches.
const COLLAPSE_CARET_REACH = COLLAPSE_CARET_WIDTH + COLLAPSE_CARET_GAP;

// The state bar, drawn past the caret sharing the gutter and clear of it by a
// pixel, so a hovered row holding an edit reads as a bar and a caret rather than
// as one mark.
const SETTING_STATE_BAR_WIDTH = 3;
const SETTING_STATE_BAR_INSET =
  COLLAPSE_CARET_REACH + SETTING_STATE_BAR_WIDTH + 1;

// The strip an array row hangs its fold in. It is the innermost of what a row
// hangs out there, so a fold is met at the same reach whether it is a setting's
// or a row's, and the grip is drawn past it.
const ARRAY_ITEM_CARET_GUTTER = `calc(${COLLAPSE_CARET_WIDTH}px + ${ARRAY_ITEM_HANDLE_OFFSET})`;

const SettingsWrapper = styled.div<{ $density: Density }>`
  ${({ $density }) => densityVariables($density)}

  // Word-wrap long lines.
  overflow-wrap: break-word;

  padding-block: var(--whui-settings-form-padding);
`;

const SettingInputNumber = styled(InputNumberWithContextMenu)`
  width: 100%;
  max-width: 130px;

  // Remove default VSCode focus highlighting color.
  input:focus {
    outline: none !important;
  }
`;

const SettingSelect = styled(SelectModal)`
  width: 100%;
`;

const SettingsCard = styled(Card)`
  width: 100%;

  .ant-card-body {
    padding: var(--whui-settings-card-padding);
  }
`;

const ArraySettingsItemContent = styled.div`
  flex: 1;
  min-width: 0;
`;

// The one line a folded row of an array of groups is left as, built like a
// folded setting's title line: what the row is called, then what it holds as a
// chip. Clicking it opens the row back out, so the box is drawn to its content
// rather than to the width of the array - the empty end of a folded row is as
// empty as it looks.
const ArrayItemSummary = styled.div`
  display: flex;
  align-items: center;
  column-gap: ${SETTING_TITLE_GAP}px;
  min-height: var(--whui-settings-array-control-height);
  width: fit-content;
  max-width: 100%;
  color: var(--whui-text-secondary);
  overflow: hidden;
  white-space: nowrap;
  cursor: pointer;
`;

// Gives up no width: the chip of what the row holds is what shrinks, as on a
// setting's title line.
const ArrayItemSummaryLabel = styled.span`
  flex-shrink: 0;
`;

// Between the controls acting on a row and the value it holds.
const ARRAY_ITEM_GUTTER = '12px';

// How long the row a move landed keeps the tint that says so, and how far the
// tint spreads past the row's own box.
const ARRAY_ITEM_MOVED_FLASH_MS = 900;
const ARRAY_ITEM_MOVED_FLASH_SPREAD = '4px';

// The grip in hand: what a pointer drag has in place of the image the browser's
// own drag and drop would draw. It comes up in the grip's own box, at the place
// the grip stood, and the pointer keeps where in it the press landed - so the
// drag begins with nothing moving.
//
// It answers no pointer of its own: sitting under the one carrying the drag, a
// badge that could be hit would be hit on every move. That is also why it is
// portaled outside the form - a fixed box is placed against the viewport only
// until an ancestor picks up a transform.
const ArrayItemDragBadge = styled.div`
  position: fixed;
  inset-block-start: 0;
  inset-inline-start: 0;
  z-index: 1000;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--whui-border);
  border-radius: 4px;
  background-color: var(--whui-float-bg);
  box-shadow: 0 2px 6px rgba(0, 0, 0, 0.24);
  color: var(--whui-text-secondary);
  pointer-events: none;
`;

// Padding rather than a width, and narrower at the compact density: antd draws a
// shorter button there, and the full-size padding would leave it wider than tall.
//
// It stands whether or not the pointer is near - what it opens is the only way a
// keyboard or a screen reader has to move a row or take it away.
const ArraySettingsDropdownOptionsButton = styled(Button)`
  padding-inline: var(--whui-settings-array-menu-padding);
`;

// The grip a row is carried by. Dragging is the one thing it does, so the cursor
// says that and nothing else, and pressing it acts on none of the buttons beside
// it.
//
// It is taken out of the flow and hung outside the row, past the fold caret, in
// the padding the card body already leaves: a row that can be reordered is laid
// out to the pixel like one that cannot. It draws over whatever is out there -
// the state bar, and past the card's gutter the surface behind it - as something
// above the form, which is what the border and the shadow say.
//
// Hidden rather than transparent until the row is hovered: a target drawn
// nowhere is never one that can still be hit. It takes no focus and says nothing
// to a screen reader, there being no way to carry a row by it but to drag it -
// the keyboard path is the menu's Move up and Move down.
const ArraySettingsItemDragHandle = styled.div`
  position: absolute;
  inset-inline-end: 100%;
  // How far out its own row puts it, which is past the fold on a row that has
  // one. Read off the row rather than matched by a selector reaching down from
  // it: an array nested in a row of an array puts grips below a foldable row
  // that answer to their own row, and one reached for as a descendant would be
  // pushed out to clear a caret standing beside something else.
  margin-inline-end: var(--whui-settings-array-grip-offset);
  top: 0;
  height: 100%;
  width: var(--whui-settings-array-handle-width);
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--whui-border);
  border-radius: 4px;
  background-color: var(--whui-float-bg);
  box-shadow: 0 2px 6px rgba(0, 0, 0, 0.24);
  color: var(--whui-text-secondary);
  visibility: hidden;
  opacity: 0;
  transition: opacity 120ms ease-out;
  cursor: grab;
  // A press carries the row, so the browser is told to do nothing else with it:
  // no panning under a touch, no selection swept through the form under a mouse.
  touch-action: none;
  user-select: none;

  &:active {
    cursor: grabbing;
  }
`;

// What acts on a row rather than holds its value. Only the menu is in the box;
// the fold and the grip hang outside it, where they cost the row nothing. The
// gap is tighter than the form's other pairs use: this is one cluster acting on
// one row, not two things side by side. It stays put through a fold - a control
// that stepped out from under the pointer that just pressed it would be asking
// to be pressed again before it could be read.
const ArraySettingsItemControls = styled.div`
  position: relative;
  // A flex box, so its height is the buttons' exactly - what the grip and the
  // caret hanging off it are measured against.
  display: flex;
  gap: 4px;
  align-self: flex-start;
  flex-shrink: 0;
`;

// Which edge of the row under the pointer the dragged one would come to rest
// against.
type DropEdge = 'before' | 'after';

// What the page is while a row is being carried across it. A pointer drag is not
// one the browser draws a cursor for, so the page says the row is in hand
// wherever the pointer has got to, and turns off the selection a pointer
// sweeping across a form of inputs would otherwise drag through it.
//
// Nothing under the pointer answers, so nothing the drag crosses draws as
// hovered: a drag is one gesture, and a form lighting up row by row reads as
// several. Only what the drag itself has to hear from is put back - the rows and
// the blocks they are drawn in, so the pointer is read as being over a row
// rather than over whichever button of it it happens to be on.
const ArrayItemDragCursor = createGlobalStyle`
  html,
  body,
  body * {
    cursor: grabbing !important;
    user-select: none !important;
  }

  body * {
    pointer-events: none !important;
  }

  [data-array-row],
  [data-array-item] {
    pointer-events: auto !important;
  }
`;

// The tint a row wears for a moment after a move lands it there. Two names for
// one flash: a CSS animation restarts only when its name changes, so a row moved
// onto twice over needs the other name to say so a second time.
const arrayItemMovedFlash = () => keyframes`
  from {
    background-color: var(--whui-setting-moved-bg);
    box-shadow: 0 0 0 ${ARRAY_ITEM_MOVED_FLASH_SPREAD} var(--whui-setting-moved-bg);
  }
  to {
    background-color: transparent;
    box-shadow: 0 0 0 ${ARRAY_ITEM_MOVED_FLASH_SPREAD} transparent;
  }
`;
const ARRAY_ITEM_MOVED_FLASHES = [arrayItemMovedFlash(), arrayItemMovedFlash()];

// A drop line rather than a live preview: the rows stay put and only the line
// says what the release would do. It is drawn in the gap between two rows, half
// on either side of the edge, so the same line means the same place whichever
// row the pointer is over.
//
// What state a row is in - carried, landed on, just moved, reorderable at all -
// is read off the attributes the row already carries for the rest of the form to
// find it by, rather than off props of its own, so one place says it and it is
// the place a test or a stylesheet can see.
const ArraySettingsItemWrapper = styled.div`
  position: relative;
  display: flex;
  gap: ${ARRAY_ITEM_GUTTER};
  border-radius: 2px;

  // What the row hangs outside itself, and how far. Every row sets both for its
  // own, so a row nested inside a foldable one reads its own numbers rather than
  // inheriting the room that row's caret asked for.
  --whui-settings-array-grip-offset: ${ARRAY_ITEM_HANDLE_OFFSET};
  // How far the row reaches out: far enough to cover the grip wherever it is
  // drawn, which is past the fold on a row that has one.
  --whui-settings-array-row-reach: 0px;

  &[data-array-item-foldable] {
    --whui-settings-array-grip-offset: calc(
      ${ARRAY_ITEM_CARET_GUTTER} + ${ARRAY_ITEM_HANDLE_OFFSET}
    );
  }

  &[data-array-item-reorderable] {
    --whui-settings-array-row-reach: var(--whui-settings-array-handle-gutter);
  }

  &[data-array-item-reorderable][data-array-item-foldable] {
    --whui-settings-array-row-reach: calc(
      var(--whui-settings-array-handle-gutter) + ${ARRAY_ITEM_CARET_GUTTER}
    );
  }

  // Reaching out takes the gap with it, so pointing at the grip is pointing at
  // the row and a drag carried straight down the strip is carried down the list.
  // The padding hands the room back to the content, laid out where it was.
  margin-inline-start: calc(-1 * var(--whui-settings-array-row-reach));
  padding-inline-start: var(--whui-settings-array-row-reach);

  // The row, not any one control in it, is what brings its own grip out - which
  // is what says whose the grip drawn outside the row is. An array nested in an
  // array puts several rows under the pointer at once and only the innermost
  // means anything, so the grip is reached for as this row's own child and a row
  // holding a hovered row answers for neither.
  //
  // Three states hold that back. A drag in flight: a grip coming out on every
  // row it crosses would say a row can be taken hold of while one already has
  // been. The stretch after a drop, where the browser still draws the row the
  // drag began on as hovered: there the row worked out to be under the pointer
  // shows its grip outright, so a released drag leaves one where the pointer is.
  // Both are read off the attributes the row carries. Third, a row with its menu
  // open, which the pointer has had to leave - read off the trigger antd marks
  // rather than held as a place in the array, since the menu's own edits move the
  // rows around and a place would name a different row the moment one landed.
  &:not([data-hover-held]):hover:not(:has([data-array-item]:hover))
    > ${ArraySettingsItemControls}
    > ${ArraySettingsItemDragHandle},
  &[data-hover-shown]
    > ${ArraySettingsItemControls}
    > ${ArraySettingsItemDragHandle},
  &:has(> ${ArraySettingsItemControls} > [data-array-item-menu].ant-dropdown-open)
    > ${ArraySettingsItemControls}
    > ${ArraySettingsItemDragHandle} {
    visibility: visible;
    opacity: 1;
  }

  // The row's fold comes out on the same terms, and is held back on the same
  // three.
  &:not([data-hover-held]):hover:not(:has([data-array-item]:hover))
    > ${ArraySettingsItemControls}
    > [data-array-item-collapse],
  &[data-hover-shown]
    > ${ArraySettingsItemControls}
    > [data-array-item-collapse],
  &:has(> ${ArraySettingsItemControls} > [data-array-item-menu].ant-dropdown-open)
    > ${ArraySettingsItemControls}
    > [data-array-item-collapse] {
    opacity: 1;
  }

  &[data-array-item-dragging] {
    opacity: 0.4;

    > ${ArraySettingsItemControls} > ${ArraySettingsItemDragHandle} {
      visibility: visible;
      opacity: 1;
    }
  }

  &[data-array-item-drop-edge]::after {
    content: '';
    position: absolute;
    // Along the row's content, not the strip it reaches out over.
    inset-inline-start: var(--whui-settings-array-row-reach);
    inset-inline-end: 0;
    height: 2px;
    border-radius: 1px;
    background-color: var(--whui-primary);
  }

  &[data-array-item-drop-edge='before']::after {
    top: -1px;
  }

  &[data-array-item-drop-edge='after']::after {
    bottom: -1px;
  }

  // Which of the two flashes a row wears is which of them it is not wearing
  // already, so a row moved onto twice over is told to flash a second time.
  &[data-array-item-moved='0'] {
    animation: ${ARRAY_ITEM_MOVED_FLASHES[0]} ${ARRAY_ITEM_MOVED_FLASH_MS}ms ease-out;
  }

  &[data-array-item-moved='1'] {
    animation: ${ARRAY_ITEM_MOVED_FLASHES[1]} ${ARRAY_ITEM_MOVED_FLASH_MS}ms ease-out;
  }
`;

// The array's own controls, under its rows: one to lengthen it, one to empty it.
// Emptying is the array's to do rather than any row's, so it is asked for here.
// Two buttons side by side, not one cluster acting on one thing, so they stand
// apart by more than a row's own controls do.
const ArraySettingsFooter = styled.div`
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
`;

// The two states a row is marked for, and it can be in both: holding something
// other than the value its mod declares, and holding an unsaved edit. Marked by
// a bar in the gutter, which reads at a glance down a long form and costs the
// title line no room. Color alone carries no meaning - the row also names its
// state to a screen reader.
type SettingState = 'non-default' | 'unsaved' | 'both';

// The bar is inset by the row's own padding, so it spans the content it marks
// rather than running halfway into the gap after it. Which padding that is, is
// the row's to say: a row of an array of plain values is set tighter.
const SettingsListItem = styled(List.Item)`
  --whui-settings-item-padding: var(--whui-settings-row-padding);

  &[data-value-row] {
    --whui-settings-item-padding: var(--whui-settings-value-row-padding);
  }

  padding-block: var(--whui-settings-item-padding);

  &:first-child {
    padding-top: 0;
  }

  &:last-child {
    padding-bottom: 0;
  }

  // The fold comes out when the pointer is on the row it acts on, so a form of
  // groups and arrays is a form rather than a column of carets. Reached through
  // the row's own title line, not as any descendant: a group holds rows carrying
  // carets of their own, and pointing at the group is not pointing at any.
  &:hover > .ant-list-item-meta [data-setting-collapse] {
    opacity: 1;
  }

  // The state the row names itself with draws its bar. An unsaved edit is the
  // one there is something to do about, so it takes the bar whenever it is
  // there - which is what the later of the two colors says.
  &[data-setting-state] {
    position: relative;

    &::before {
      content: '';
      position: absolute;
      inset-block: var(--whui-settings-item-padding);
      inset-inline-start: -${SETTING_STATE_BAR_INSET}px;
      width: ${SETTING_STATE_BAR_WIDTH}px;
      border-radius: 2px;
      background-color: var(--whui-setting-unsaved);
    }

    &:first-child::before {
      top: 0;
    }

    &:last-child::before {
      bottom: 0;
    }
  }

  &[data-setting-state='non-default']::before {
    background-color: var(--whui-setting-non-default);
  }
`;

// Text for assistive technology only, clipped out of the visual layout.
const VisuallyHidden = styled.span`
  position: absolute;
  width: 1px;
  height: 1px;
  margin: -1px;
  padding: 0;
  overflow: hidden;
  clip-path: inset(50%);
  white-space: nowrap;
  border: 0;
`;

// The title sits in an <h4>, whose content model is phrasing content only, so
// the row of title, mark and reset control is laid out on inline elements.
//
// Centered rather than baseline-aligned: the parts share a line box but not a
// font size, and aligning their baselines would offset the boxes and make the
// row a pixel taller than an unmarked one.
//
// The line does not wrap. A title too long for the row wraps within itself, and
// the reset control gives up width rather than dropping to a line of its own.
const SettingTitleWrapper = styled.span`
  display: flex;
  align-items: center;
  column-gap: ${SETTING_TITLE_GAP}px;
`;

// The name gives up no width at all - a pixel less than it asks for wraps its
// last word onto a line of its own - so the reset control beside it shrinks
// instead. The cap keeps a name longer than the row wrapping within itself.
//
// A name with a form behind it folds it, as the caret does: pressing what a row
// is called asks for what is under it. The rest of the line acts on the value,
// so a press that missed one of those is a press that missed.
const SettingTitleText = styled.span<{ $foldable?: boolean }>`
  flex-shrink: 0;
  max-width: 100%;

  ${({ $foldable }) =>
    $foldable &&
    css`
      cursor: pointer;
    `}
`;

// What a folded row says it is holding: a row folded to its name alone reads as
// a row with nothing in it, and the count tells the two apart. A chip rather
// than more of the title line, since what it counts is not part of the name.
//
// It gives up width before the title does, and is cut short rather than wrapped.
const SettingSummary = styled.span`
  flex-shrink: 1;
  min-width: 0;
  padding-inline: 8px;
  border-radius: 10px;
  background-color: var(--whui-chip-bg);
  color: var(--whui-text-secondary);
  font-size: 12px;
  font-weight: normal;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
`;

// How much of the title line the reset control may take before its label is cut
// short. A width rather than a count of characters: the same number of letters
// can draw at twice the width. The whole of it stays readable as the tooltip.
const MAX_RESET_LABEL_WIDTH = '260px';

// A control sharing the title's line, sized to that line box and no more - a
// taller one would push a marked row's title against its neighbors. So antd's
// own height and its transparent border go, which together make a small button
// two pixels taller than the line. The target is widened with padding instead,
// which costs the line box nothing, and pulled back by a margin so what it draws
// still lines up where the title's column gap puts it.
const inlineTitleControl = css`
  display: inline-flex;
  align-items: center;
  height: auto;
  min-height: 0;
  line-height: inherit;
  border: 0;
  padding: 0 8px;
  margin-inline-start: -8px;
  font-size: 12px;
  font-weight: normal;
`;

// Small print next to the title, and the part of the line that gives way when
// the row cannot hold both: min-width lets it shrink past its label and the
// shrink factor makes it take practically the whole shortfall, so a narrow
// window cuts the label down instead of wrapping the title over several lines.
const ResetSettingButton = styled(Button)`
  ${inlineTitleControl}
  flex-shrink: 100;
  min-width: 0;
  max-width: ${MAX_RESET_LABEL_WIDTH};
`;

// The line breaks in a description are the mod author's, and are kept wherever
// it is shown. Runs of spaces still collapse: what is preserved is how the text
// was broken into lines, not its indentation.
const descriptionText = css`
  white-space: pre-line;
`;

// Wider than antd's 250px, which is a caption's width and not a paragraph's -
// this carries the mod author's whole account of a setting. It gives way to a
// narrow panel, there being nowhere to draw a tooltip wider than the window.
const SETTING_DESCRIPTION_MAX_WIDTH = 'min(520px, 90vw)';

// The (i) opening a description a compact row has no room to print. It holds its
// width: a glyph squeezed to nothing opens nothing.
const SettingDescriptionButton = styled(Button)`
  ${inlineTitleControl}
  flex-shrink: 0;
  color: var(--whui-text-secondary);
`;

// What every fold in the form is drawn as: the same glyph at the same size,
// turning the same way, at the start of what it folds.
//
// Transparent rather than hidden until the pointer is on the row it belongs to -
// a caret beside every group turns a form into an outline to be worked through
// first - so it keeps its place at either state and a keyboard reaches it as it
// reaches any other button, which draws it too. A row already folded is the
// exception: there the caret is the only thing saying it has anything behind it.
//
// The box is the glyph's, but what can be aimed at is not: a pseudo-element
// spreads the target into space no line was using, at no cost to the layout.
const collapseCaret = css<{ $collapsed: boolean; $rtl: boolean }>`
  ${inlineTitleControl}
  position: relative;
  flex-shrink: 0;
  min-width: 0;
  width: ${COLLAPSE_CARET_WIDTH}px;
  padding: 0;
  margin-inline-start: 0;
  justify-content: center;
  color: var(--whui-text-secondary);
  opacity: ${({ $collapsed }) => ($collapsed ? 1 : 0)};
  transition: opacity 120ms ease-out;

  &::before {
    content: '';
    position: absolute;
    inset: -4px -6px;
  }

  &:focus-visible {
    opacity: 1;
  }

  // One glyph turned rather than two swapped, so the caret moves between the two
  // states instead of jumping. It turns the way the text runs, so a folded row
  // points the way its content would open out.
  svg {
    transition: transform 120ms ease-out;

    ${({ $collapsed, $rtl }) =>
      $collapsed &&
      css`
        transform: rotate(${$rtl ? '90deg' : '-90deg'});
      `}
  }
`;

// The caret folding away what a group or an array holds - the one thing on the
// title line that says nothing about the setting's value.
//
// It hangs in the gutter rather than standing in the line, so titles line up
// down the form whether or not there is anything behind them. Pulled out of the
// line by everything it reaches and given back all but its own gap, so the title
// follows where it would have with no caret there at all. The gutter is the
// row's own at every depth, which steps the carets in with the nesting.
const SettingCollapseButton = styled(Button) <{ $collapsed: boolean; $rtl: boolean }>`
  ${collapseCaret}
  margin-inline-start: -${COLLAPSE_CARET_REACH}px;
  margin-inline-end: -${SETTING_TITLE_GAP - COLLAPSE_CARET_GAP}px;
`;

// The caret folding away one row of an array of groups. A row has no title line
// to hang it off, so it hangs off the start of the row - taken out of the flow
// the way a setting's caret is taken out of its title line, so a row that folds
// is laid out to the pixel like a row that cannot. The grip, where the row
// carries one, is drawn past it.
const ArrayItemCollapseButton = styled(Button) <{
  $collapsed: boolean;
  $rtl: boolean;
}>`
  ${collapseCaret}
  position: absolute;
  inset-inline-end: 100%;
  margin-inline-end: ${ARRAY_ITEM_HANDLE_OFFSET};
  top: 0;
  // The height of the line it hangs off, so the glyph is centered on it and on
  // the menu beside it rather than on a box the size of the glyph.
  height: var(--whui-settings-array-control-height);
`;

// antd draws a tooltip's body from the node it is handed, so the text carries
// its own rule rather than the app's tooltips all carrying one.
const SettingDescriptionText = styled.span`
  ${descriptionText}
`;

const ResetSettingIcon = styled(FontAwesomeIcon)`
  flex-shrink: 0;
  margin-inline-end: 6px;
`;

const ResetSettingLabel = styled.span`
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
`;

const SettingsListItemMeta = styled(List.Item.Meta)`
  .ant-list-item-meta {
    margin-bottom: var(--whui-settings-meta-margin);
  }

  .ant-list-item-meta-title {
    margin-bottom: 0;
  }

  .ant-list-item-meta-description {
    ${descriptionText}
  }
`;

// Fullscreen is a fixed overlay that scrolls as a whole, with the action bar
// pinned by the same position: sticky it already uses inside the panel, so the
// inner layout is shared between the two modes. The side and bottom inset mirror
// the outer card body padding; the top is left to the toolbar, which pins flush.
const SettingsForm = styled.form<{ $fullscreen: boolean }>`
  ${({ $fullscreen }) =>
    $fullscreen &&
    css`
      position: fixed;
      inset: 0;
      z-index: 100;
      overflow-y: auto;
      padding: 0 24px 24px;
      background-color: var(--whui-card-background-color);
    `}
`;

const SaveSettingsCard = styled(Card) <{ $fullscreen: boolean }>`
  position: sticky;
  top: 0;
  z-index: 1;
  margin-top: -12px;
  margin-inline: -24px;
  padding-inline: 12px;
  border-radius: 0;

  ${({ $fullscreen }) =>
    $fullscreen &&
    css`
      margin-top: 0;
      padding-top: 12px;
    `}
`;

// Two clusters: what acts on the settings, and what acts only on how they are
// shown - so the one control that rewrites the whole form does not sit shoulder
// to shoulder with a toggle that changes no data. The row wraps as a whole, so a
// narrow panel drops the view controls onto a line of their own.
const SaveSettingsHeader = styled.div`
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 12px;
`;

const SaveSettingsHeaderMain = styled.div`
  flex: 1;
  min-width: 0;
`;

const ToolbarButton = styled(Button)`
  flex-shrink: 0;
`;

const ToolbarGroup = styled.div`
  display: flex;
  align-items: center;
  gap: 12px;
`;

// Held to the end of the strip by the free space between the clusters, which is
// also what separates them, so neither needs a rule drawn between them.
const ViewControlsGroup = styled(ToolbarGroup)`
  margin-inline-start: auto;
`;

// The one control on the strip that gives up width, its labels cut short where a
// button would instead be pushed off the end. antd sizes the segments to their
// labels, so both it and they have to be allowed to shrink.
const ModeSegmented = styled(Segmented)`
  min-width: 0;

  .ant-segmented-item {
    min-width: 0;
  }
`;

// A button that stays on rather than acting once, so it has to look on. antd has
// no such button - what it draws for a click is a hover it drops as soon as the
// pointer leaves - so this takes that hover accent and holds it.
const ViewToggleButton = styled(ToolbarButton) <{ $pressed: boolean }>`
  ${({ $pressed }) =>
    $pressed &&
    css`
      &,
      &:hover,
      &:focus {
        color: var(--whui-primary);
        border-color: var(--whui-primary);
        background-color: var(--whui-chip-bg);
      }
    `}
`;

// ============================================================================
// Type Definitions
// ============================================================================

type InitialSettingItemExtra = {
  options?: Record<string, string>[];
};

/**
 * For read-only object arrays: overlay the annotations (name, description,
 * $options) of the array's template group onto a data row, which carries the
 * values but none of the annotations. A key the row omits falls back to the
 * template entry, so the row renders the template's whole key set.
 *
 * The overlay follows the row into its nested groups and object arrays, which
 * are just as unannotated. Every row it produces carries the template's complete
 * key set, so overlaying an already overlaid row (what a nested ArraySettings
 * does with its own element 0) is a no-op rather than a second, lossy pass.
 */
function mergeInitialSettingsMetadata(
  schema: InitialSettings,
  data: InitialSettings
): InitialSettings {
  return schema.map((schemaItem) => {
    const dataItem = data.find((d) => d.key === schemaItem.key);
    return dataItem
      ? { ...schemaItem, value: mergeValueMetadata(schemaItem.value, dataItem.value) }
      : schemaItem;
  });
}

function mergeValueMetadata(
  schemaValue: InitialSettingsValue,
  dataValue: InitialSettingsValue
): InitialSettingsValue {
  const schemaDescriptor = describeSetting(schemaValue);
  const dataDescriptor = describeSetting(dataValue);

  if (
    schemaDescriptor.kind === SettingType.NestedObject &&
    dataDescriptor.kind === SettingType.NestedObject
  ) {
    return mergeInitialSettingsMetadata(schemaDescriptor.children, dataDescriptor.value);
  }

  if (
    schemaDescriptor.kind === SettingType.ObjectArray &&
    dataDescriptor.kind === SettingType.ObjectArray
  ) {
    return dataDescriptor.value.map((row) =>
      mergeInitialSettingsMetadata(schemaDescriptor.children, row)
    );
  }

  return dataValue;
}

// ============================================================================
// Setting Components
// ============================================================================

/**
 * Whether a setting sits under an array, at any depth. The flat key syntax
 * reserves a bracket for exactly that.
 *
 * The schema declares one template per array, so a row inside one has no default
 * of its own to revert to. Reverting the array as a whole is the operation that
 * means something, and it is offered on the array's own row.
 */
function isInsideArray(settingKey: string): boolean {
  return settingKey.includes('[');
}

/**
 * Whether a setting holds a form of its own - a group, or an array of values or
 * groups - rather than a single value. Only those are worth folding: a row
 * holding one control is already one line.
 */
function isCollapsibleSetting(value: InitialSettingsValue): boolean {
  switch (describeSetting(value).kind) {
    case SettingType.NestedObject:
    case SettingType.NumberArray:
    case SettingType.StringArray:
    case SettingType.ObjectArray:
      return true;
    default:
      return false;
  }
}

/**
 * How a row is marked, or undefined when it is at its default and saved.
 *
 * A row inside an array takes no state of its own: what its whole array is in is
 * marked on the array's own row, which is also where the revert lives. Nothing
 * is marked in the read-only preview, which edits nothing.
 */
function settingState(
  { modSettings, canonicalDraft, canonicalSaved, readOnly }: SettingsTreeProps,
  value: InitialSettingsValue,
  settingKey: string
): SettingState | undefined {
  if (readOnly || isInsideArray(settingKey)) {
    return undefined;
  }

  const nonDefault = isSettingModified(modSettings, value, settingKey);
  const unsaved = isSubtreeChanged(canonicalDraft, canonicalSaved, settingKey);

  if (nonDefault && unsaved) {
    return 'both';
  }
  return nonDefault ? 'non-default' : unsaved ? 'unsaved' : undefined;
}

/**
 * What a setting drawn as a dropdown offers: the value each option stores, and
 * the label it carries. A setting that is not a dropdown offers nothing.
 */
function settingOptions(item: InitialSettingItemExtra): {
  value: string;
  label: string;
}[] {
  return (item.options ?? []).map((option) => {
    const [value, label] = Object.entries(option)[0];
    return { value, label };
  });
}

/**
 * How a stored value reads on a dropdown: the label its option carries. A value
 * no option names, and any setting that is not a dropdown, reads as itself.
 */
function optionLabel(item: InitialSettingItem, value: string): string {
  return settingOptions(item).find((option) => option.value === value)?.label ?? value;
}

/**
 * The declared default of a setting as a single line. Null for a group or an
 * array, which has no one value to name.
 */
function defaultValueLabel(item: InitialSettingItem): string | null {
  const value = formatDefaultValue(item.value);
  return value === null ? null : optionLabel(item, value);
}

/**
 * The one value a setting declares, or undefined for one that declares a form.
 * A switch reads as the number the store keeps it as, as the form reads it too.
 */
function declaredScalarValue(value: InitialSettingsValue): string | number | undefined {
  const descriptor = describeSetting(value);
  switch (descriptor.kind) {
    case SettingType.Boolean:
      return descriptor.value ? 1 : 0;
    case SettingType.Number:
    case SettingType.String:
      return descriptor.value;
    default:
      return undefined;
  }
}

// Enough fields to tell one folded row from the next, few enough to stay on the
// one line it is given.
const ARRAY_ROW_SUMMARY_FIELDS = 3;

/**
 * What a folded row of an array of groups is left showing: the values its fields
 * hold, in the order the mod declares them.
 *
 * Only fields holding one value are named, and a blank one is passed over, so a
 * half filled row reads as what it holds rather than as a run of separators. A
 * dropdown is named by its option's label, and a switch by the field's own name,
 * which says more than the 1 the store keeps it as.
 */
function rowSummaryValues(
  children: InitialSettings,
  valueOf: (child: InitialSettingItem) => string | number | undefined
): string[] {
  const values: string[] = [];

  for (const child of children) {
    if (values.length === ARRAY_ROW_SUMMARY_FIELDS) {
      break;
    }

    const value = valueOf(child);
    if (value === undefined || value === '') {
      continue;
    }

    switch (describeSetting(child.value).kind) {
      case SettingType.Boolean:
        if (parseIntLax(value)) {
          values.push(child.name || child.key);
        }
        break;

      case SettingType.Number:
        values.push(value.toString());
        break;

      case SettingType.String:
        values.push(optionLabel(child, value.toString()));
        break;

      default:
        break;
    }
  }

  return values;
}

// How far an array reaches and what of it is filled in. An array nobody has
// filled in still draws one row, which is a place to type in rather than an
// element it has - which is what hasItems tells apart.
type ArrayExtent = {
  maxIndex: number;
  lastItemEmpty: boolean;
  hasItems: boolean;
};

function arrayExtent(
  { modSettings, canonicalDraft, arrayItemMaxIndex, readOnly }: SettingsTreeProps,
  keyPrefix: string,
  declaredLength: number
): ArrayExtent {
  const maxIndex = Math.max(
    materializedMaxIndex(modSettings, keyPrefix),
    arrayItemMaxIndex[keyPrefix] ?? 0,
    readOnly ? declaredLength - 1 : -1
  );

  // What the last row holds is read off the canonical draft: a row cleared back
  // out holds nothing, whatever key the edit left behind.
  const lastItemEmpty = materializedMaxIndex(canonicalDraft, keyPrefix) < maxIndex;

  return { maxIndex, lastItemEmpty, hasItems: maxIndex > 0 || !lastItemEmpty };
}

// What each marked state is called, for the reader that has the row's words but
// not the color of its bar.
const SETTING_STATE_LABEL: Record<SettingState, string> = {
  'non-default': 'modDetails.settings.modified',
  unsaved: 'modDetails.settings.unsaved',
  both: 'modDetails.settings.modifiedUnsaved',
};

interface SettingDescriptionProps {
  name: string;
  description: string;
}

/**
 * A setting's description, off the row and behind the (i) beside its name.
 *
 * The text itself stays in the row, out of sight rather than out of the
 * document, so a screen reader going down the form gets the same words in the
 * same place at either density.
 */
function SettingDescription({ name, description }: SettingDescriptionProps) {
  const { t } = useTranslation();
  const descriptionId = useId();

  return (
    <>
      <Tooltip
        title={
          <SettingDescriptionText data-testid="mod-setting-description-text">
            {description}
          </SettingDescriptionText>
        }
        trigger={['hover', 'focus']}
        placement="bottom"
        overlayStyle={{ maxWidth: SETTING_DESCRIPTION_MAX_WIDTH }}
      >
        {/* Nothing to click: the description is already showing by the time a
            press lands, and the press would leave the button focused - drawn as
            such, holding the tooltip open - after the pointer has gone. Only the
            focus a mouse press gives is declined; a keyboard still focuses it,
            which is what opens the description there. */}
        <SettingDescriptionButton
          type="link"
          size="small"
          aria-label={t('modDetails.settings.descriptionOf', { name })}
          aria-describedby={descriptionId}
          data-testid="mod-setting-description"
          onMouseDown={(event) => event.preventDefault()}
        >
          <FontAwesomeIcon icon={faCircleInfo} />
        </SettingDescriptionButton>
      </Tooltip>
      <VisuallyHidden id={descriptionId}>{description}</VisuallyHidden>
    </>
  );
}

// Where the caret is drawn, which is the whole of what tells the two apart: on a
// setting's title line, or off the start of an array's row.
type CollapseCaretPlace = 'setting' | 'arrayItem';

const COLLAPSE_CARET_MARKERS = {
  setting: { 'data-setting-collapse': '', 'data-testid': 'mod-setting-collapse' },
  arrayItem: {
    'data-array-item-collapse': '',
    'data-testid': 'mod-setting-array-item-collapse',
  },
} as const;

interface CollapseCaretProps {
  place: CollapseCaretPlace;
  collapsed: boolean;
  // Already saying which state it is in and what it acts on: the caret is the
  // whole of what says so on screen, and the name is what tells one from the
  // next in a form of them.
  label: string;
  onToggle: () => void;
}

/** The control folding a form away and opening it back out. */
function CollapseCaret({ place, collapsed, label, onToggle }: CollapseCaretProps) {
  const { direction } = useContext(ConfigProvider.ConfigContext);
  const CaretButton =
    place === 'setting' ? SettingCollapseButton : ArrayItemCollapseButton;

  return (
    <CaretButton
      type="link"
      size="small"
      $collapsed={collapsed}
      $rtl={direction === 'rtl'}
      aria-expanded={!collapsed}
      aria-label={label}
      {...COLLAPSE_CARET_MARKERS[place]}
      onClick={onToggle}
    >
      <FontAwesomeIcon icon={faChevronDown} />
    </CaretButton>
  );
}

/**
 * Whether a press on text that stands in for a form is the press that folds it.
 * A press ending a selection is the selection's: reading a name by dragging
 * across it is not asking for the row to close over it.
 */
function isFoldingClick(): boolean {
  return document.getSelection()?.isCollapsed !== false;
}

function foldingClickHandler(onToggle: () => void) {
  return () => {
    if (isFoldingClick()) {
      onToggle();
    }
  };
}

interface SettingTitleProps {
  title: string;
  state?: SettingState;
  defaultLabel: string | null;
  description?: string;
  summary?: string;
  collapsed?: boolean;
  onToggleCollapse?: () => void;
  onReset: () => void;
}

/**
 * A row title and what shares its line: the fold caret, the count a folded row
 * is holding, the state of a marked setting, the description a tight row cannot
 * print, and the control putting a non-default value back. A row that is only
 * unsaved is already at its default and has nothing to revert to.
 */
function SettingTitle({
  title,
  state,
  defaultLabel,
  description,
  summary,
  collapsed,
  onToggleCollapse,
  onReset,
}: SettingTitleProps) {
  const { t } = useTranslation();

  // Rendered whole and cut off by the width it is allowed rather than by a count
  // of characters, so what is spoken stays whole.
  const resetLabel =
    defaultLabel === null
      ? t('modDetails.settings.resetToDefault')
      : t('modDetails.settings.resetToDefaultValue', { value: defaultLabel });

  const resetAriaLabel =
    defaultLabel === null
      ? t('modDetails.settings.resetToDefaultOf', { name: title })
      : t('modDetails.settings.resetToDefaultValueOf', { name: title, value: defaultLabel });

  return (
    <SettingTitleWrapper>
      {/* First of the line and hanging off the start of it: what it acts on is
          the row, not the setting's value. */}
      {onToggleCollapse && (
        <CollapseCaret
          place="setting"
          collapsed={!!collapsed}
          label={t(
            collapsed
              ? 'modDetails.settings.expandSetting'
              : 'modDetails.settings.collapseSetting',
            { name: title }
          )}
          onToggle={onToggleCollapse}
        />
      )}
      <SettingTitleText
        $foldable={!!onToggleCollapse}
        onClick={onToggleCollapse ? foldingClickHandler(onToggleCollapse) : undefined}
      >
        {title}
      </SettingTitleText>
      {/* Right after the name, so what the row holds is read with the row. */}
      {summary && (
        <SettingSummary data-testid="mod-setting-summary">{summary}</SettingSummary>
      )}
      {state && <VisuallyHidden>{t(SETTING_STATE_LABEL[state])}</VisuallyHidden>}
      {description && <SettingDescription name={title} description={description} />}
      {state && state !== 'unsaved' && (
        <ResetSettingButton
          type="link"
          size="small"
          title={defaultLabel ?? undefined}
          aria-label={resetAriaLabel}
          data-testid="mod-setting-reset"
          onClick={onReset}
        >
          <ResetSettingIcon icon={faRotateLeft} />
          <ResetSettingLabel>{resetLabel}</ResetSettingLabel>
        </ResetSettingButton>
      )}
    </SettingTitleWrapper>
  );
}

interface BooleanSettingProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
}

function BooleanSetting({ checked, onChange, disabled }: BooleanSettingProps) {
  return <Switch checked={checked} onChange={onChange} disabled={disabled} />;
}

interface StringSettingProps {
  value: string;
  sampleValue: string;
  onChange: (newValue: string) => void;
  readOnly?: boolean;
}

function StringSetting({ value, sampleValue, onChange, readOnly }: StringSettingProps) {
  const { t } = useTranslation();

  let placeholder: string | undefined;
  if (sampleValue) {
    placeholder = t('modDetails.settings.sampleValue') + `: ${sampleValue}`;
  }

  return (
    <InputWithContextMenu
      placeholder={placeholder}
      value={readOnly ? undefined : value}
      onChange={(e) => onChange(e.target.value)}
      readOnly={readOnly}
    />
  );
}

interface SelectSettingProps {
  value: string;
  sampleValue?: string;
  selectItems: {
    value: string;
    label: string;
  }[];
  onChange: (newValue: string) => void;
  readOnly?: boolean;
}

// How wide a dropdown may draw before it is left to ask for whatever its options
// need, and what those options are measured against.
const SELECT_LABEL_FONT = '14px "Segoe UI"';
const SELECT_LABEL_MAX_WIDTH = 350;
const SELECT_MAX_WIDTH = '400px';

// Made once and kept: every dropdown measures on every render, and a canvas per
// ask is an element and a drawing context built to read one number off.
let labelMeasureContext: CanvasRenderingContext2D | null | undefined;

function measureLabelWidth(label: string): number | undefined {
  if (labelMeasureContext === undefined) {
    labelMeasureContext = document.createElement('canvas').getContext('2d');
    if (labelMeasureContext) {
      labelMeasureContext.font = SELECT_LABEL_FONT;
    }
  }

  return labelMeasureContext?.measureText(label).width;
}

function SelectSetting({
  value,
  sampleValue,
  selectItems,
  onChange,
  readOnly,
}: SelectSettingProps) {
  // Held to a width its options fit in, and let out of it by one too long.
  const maxWidth = selectItems.every((item) => {
    const width = measureLabelWidth(item.label);
    return width !== undefined && width <= SELECT_LABEL_MAX_WIDTH;
  })
    ? SELECT_MAX_WIDTH
    : undefined;

  let placeholder: string | undefined;
  if (readOnly) {
    placeholder = selectItems.find((item) => item.value === sampleValue)?.label;
  }

  return (
    <div style={{ maxWidth }}>
      <SettingSelect
        showSearch={!readOnly}
        optionFilterProp="children"
        listHeight={240}
        value={readOnly ? undefined : value}
        placeholder={placeholder}
        onChange={(newValue) => {
          if (!readOnly) {
            onChange(newValue as string);
          }
        }}
      >
        {selectItems.map((item) => (
          <Select.Option key={item.value} value={item.value} disabled={readOnly}>
            {item.label}
          </Select.Option>
        ))}
      </SettingSelect>
    </div>
  );
}

interface NumberSettingProps {
  value: number;
  sampleValue?: number;
  onChange: (newValue: number) => void;
  readOnly?: boolean;
}

function NumberSetting({ value, sampleValue, onChange, readOnly }: NumberSettingProps) {
  let placeholder: string | undefined;
  if (readOnly) {
    placeholder = parseIntLax(sampleValue).toString();
  }

  return (
    <SettingInputNumber
      value={readOnly ? undefined : value}
      min={INT32_MIN}
      max={INT32_MAX}
      onChange={(newValue) => onChange(parseIntLax(newValue))}
      readOnly={readOnly}
      placeholder={placeholder}
    />
  );
}

// ============================================================================
// Settings Tree Components
// ============================================================================

interface SettingsTreeProps {
  modSettings: ModSettings;
  density: Density;
  // The draft and the saved baseline in canonical form, which is what an unsaved
  // edit is judged against: there a value cleared to its type's zero reads the
  // way an unset one does. Neither is what the form renders from - that is
  // modSettings, the draft as it is.
  canonicalDraft: ModSettings;
  canonicalSaved: ModSettings;
  onSettingChanged: (key: string, newValue: string | number) => void;
  arrayItemMaxIndex: Record<string, number>;
  onRemoveArrayItem: (key: string, index: number) => void;
  onRemoveAllArrayItems: (key: string) => void;
  onMoveArrayItem: (key: string, from: number, to: number) => void;
  onNewArrayItem: (key: string, index: number) => void;
  onResetSetting: (key: string) => void;
  // Which settings are folded away, by key. Every level of the tree reads the
  // one set: a group is folded by the same control whether it is a setting of
  // the mod or a member of a row of an array.
  collapsedKeys: ReadonlySet<string>;
  onToggleCollapsed: (key: string) => void;
  readOnly?: boolean;
}

// What the form is drawn from and what an edit goes through. Put up once around
// the whole tree rather than handed down level by level: every level reads the
// same thing, and a row nested four deep asked the three above it for nothing
// but a way through.
const SettingsTreeContext = createContext<SettingsTreeProps | null>(null);

function useSettingsTree(): SettingsTreeProps {
  const settingsTree = useContext(SettingsTreeContext);
  if (!settingsTree) {
    throw new Error('A settings row is drawn inside the settings tree it reads');
  }
  return settingsTree;
}

interface SingleSettingProps {
  initialSettingsValue: InitialSettingsValue;
  initialSettingItemExtra?: InitialSettingItemExtra;
  settingKey: string;
}

function SingleSetting({
  initialSettingsValue,
  initialSettingItemExtra,
  settingKey,
}: SingleSettingProps) {
  const { modSettings, onSettingChanged, readOnly } = useSettingsTree();
  const descriptor = describeSetting(initialSettingsValue);

  switch (descriptor.kind) {
    case SettingType.Boolean:
      return (
        <BooleanSetting
          checked={readOnly ? descriptor.value : !!parseIntLax(modSettings[settingKey])}
          onChange={(checked) => onSettingChanged(settingKey, checked ? 1 : 0)}
          disabled={readOnly}
        />
      );

    case SettingType.Number:
      return (
        <NumberSetting
          value={parseIntLax(modSettings[settingKey])}
          sampleValue={descriptor.value}
          onChange={(newValue) => onSettingChanged(settingKey, newValue)}
          readOnly={readOnly}
        />
      );

    case SettingType.String:
      if (initialSettingItemExtra?.options) {
        return (
          <SelectSetting
            value={(modSettings[settingKey] ?? '').toString()}
            sampleValue={descriptor.value}
            selectItems={settingOptions(initialSettingItemExtra)}
            onChange={(newValue) => onSettingChanged(settingKey, newValue)}
            readOnly={readOnly}
          />
        );
      }
      return (
        <StringSetting
          value={(modSettings[settingKey] ?? '').toString()}
          sampleValue={descriptor.value}
          onChange={(newValue) => onSettingChanged(settingKey, newValue)}
          readOnly={readOnly}
        />
      );

    case SettingType.NumberArray:
    case SettingType.StringArray:
    case SettingType.ObjectArray:
      return (
        <ArraySettings
          initialSettingsItems={descriptor.value}
          initialSettingItemExtra={initialSettingItemExtra}
          keyPrefix={settingKey}
          itemsAreValues={descriptor.kind !== SettingType.ObjectArray}
        />
      );

    case SettingType.NestedObject:
      return (
        <SettingsCard>
          <ObjectSettings
            initialSettings={descriptor.value}
            keyPrefix={settingKey + '.'}
          />
        </SettingsCard>
      );
  }
}

interface ArraySettingsProps {
  initialSettingsItems: InitialSettingsArrayValue;
  initialSettingItemExtra?: InitialSettingItemExtra;
  keyPrefix: string;
  // Whether a row of this array holds a single value rather than a group. A row
  // of values is one line of one setting, set tight against the lines around it
  // and with nothing to fold; a row of a group is a form of its own.
  itemsAreValues: boolean;
}

// A drag in flight: the row it picked up, the row the pointer is over, the box
// the grip filled when the press landed, and where in that box it landed. The
// one places the badge to begin with, the other holds it against the pointer.
//
// Each array keeps its own, so a row dragged out of one finds nowhere to land in
// another - an element belongs to the array it was declared in.
type ArrayItemDrag = {
  from: number;
  over: number;
  grip: { x: number; y: number; width: number; height: number };
  grab: { x: number; y: number };
};

// Where the grip in hand is drawn for a pointer at a point of the screen: the
// place that keeps the pointer where in the grip it took hold.
function arrayItemDragBadgeTransform(
  x: number,
  y: number,
  grab: { x: number; y: number }
) {
  return `translate(${x - grab.x}px, ${y - grab.y}px)`;
}

// How near the edge the pointer has to come for the box to start following it,
// and how far it travels per frame with the pointer right at the edge. A drag
// holds the pointer for as long as it lasts, so an array running past the bottom
// of the view has no other way to be scrolled.
const DRAG_SCROLL_EDGE = 48;
const DRAG_SCROLL_MAX_STEP = 14;

// The box a point on the screen would scroll: the nearest ancestor that both
// may scroll and has somewhere to scroll to.
function scrollingAncestor(element: Element | null) {
  for (let node = element?.parentElement ?? null; node; node = node.parentElement) {
    const { overflowY } = getComputedStyle(node);
    if (
      (overflowY === 'auto' || overflowY === 'scroll') &&
      node.scrollHeight > node.clientHeight
    ) {
      return node;
    }
  }
  return null;
}

// Scrolls the box a drag is happening in while the pointer is held near its top
// or bottom edge, faster the nearer the edge it is held.
function useDragScroll() {
  const scroller = useRef<HTMLElement | null>(null);
  const step = useRef(0);
  const frame = useRef(0);

  // Runs for as long as the drag does, taking whatever step the pointer's last
  // place asked for - which is no step at all most of the time.
  const advance = () => {
    if (scroller.current && step.current !== 0) {
      scroller.current.scrollTop += step.current;
    }
    frame.current = requestAnimationFrame(advance);
  };

  const stop = () => {
    cancelAnimationFrame(frame.current);
    frame.current = 0;
    scroller.current = null;
    step.current = 0;
  };

  useEffect(
    () => () => {
      cancelAnimationFrame(frame.current);
    },
    []
  );

  return {
    // Looked up once, where the drag starts: a row only ever lands in the array
    // it was picked up from, and that array is in one box.
    begin(element: Element | null) {
      scroller.current = scrollingAncestor(element);
      step.current = 0;
      if (scroller.current) {
        frame.current = requestAnimationFrame(advance);
      }
    },
    // Where the pointer has got to over the array, or null for one that has left
    // it: the box is scrolled to bring more of the array under the pointer, and
    // a pointer off the array has no more of it to bring under.
    //
    // The step is worked out against the box rather than what is under the
    // pointer, so held past either edge it is the same step as held at it.
    track(pointerY: number | null) {
      if (!scroller.current) {
        return;
      }

      if (pointerY === null) {
        step.current = 0;
        return;
      }

      const { top, bottom } = scroller.current.getBoundingClientRect();
      const intoTop = DRAG_SCROLL_EDGE - (pointerY - top);
      const intoBottom = DRAG_SCROLL_EDGE - (bottom - pointerY);
      const reach = Math.min(1, Math.max(intoTop, intoBottom) / DRAG_SCROLL_EDGE);

      step.current =
        reach <= 0
          ? 0
          : Math.ceil(reach * DRAG_SCROLL_MAX_STEP) * (intoTop > intoBottom ? -1 : 1);
    },
    end: stop,
  };
}

function ArraySettings({
  initialSettingsItems,
  initialSettingItemExtra,
  keyPrefix,
  itemsAreValues,
}: ArraySettingsProps) {
  const { t } = useTranslation();

  const settingsTree = useSettingsTree();
  const {
    modSettings,
    onRemoveArrayItem,
    onRemoveAllArrayItems,
    onMoveArrayItem,
    onNewArrayItem,
    collapsedKeys,
    onToggleCollapsed,
    readOnly,
  } = settingsTree;

  const [drag, setDrag] = useState<ArrayItemDrag | null>(null);
  const dragScroll = useDragScroll();

  // What the drag's listeners read it off. They are hung off the document rather
  // than off React, so they see whatever was last written here rather than
  // whatever a render closed over.
  const dragRef = useRef<ArrayItemDrag | null>(null);
  // How a drag in flight is called off from outside it, which is what a settings
  // form unmounted mid-drag has to do.
  const endDragRef = useRef<(() => void) | null>(null);

  // The badge is moved by hand rather than redrawn from state: one that followed
  // the pointer through React would redraw the whole array on every move, and a
  // move changes nothing else on screen. Only the moves, though - where it
  // starts is the press, which has to be state. The two do not fight over it,
  // the press point being the same at every render of one drag.
  const dragBadgeRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => () => endDragRef.current?.(), []);

  const setDragTo = (next: ArrayItemDrag | null) => {
    dragRef.current = next;
    setDrag(next);
  };

  // Where the pointer is once a drop has landed, for as long as the browser has
  // it wrong. A drag sends no mouseover, so a drop leaves :hover on the row the
  // drag began at - one that has since moved elsewhere - and nothing is
  // recomputed until the pointer moves again. Over that stretch the browser's
  // hover is held back and this row shows its grip instead, so a released drag
  // leaves one where the pointer is rather than none anywhere.
  //
  // Named by the element the drop landed on: a row is drawn at the place its
  // index puts it, so that is the row now under the pointer whatever it holds.
  // Nothing names it when a drag is called off, or let go where no row is.
  const [staleHover, setStaleHover] = useState<{ row: string | null } | null>(null);

  const arrayItemAt = (target: EventTarget | null) =>
    (target instanceof Element ? target.closest('[data-array-item]') : null)?.getAttribute(
      'data-array-item'
    ) ?? null;

  // The row a move just landed, and which of the two flashes says so. A row can
  // be moved to from off the screen, or by a menu item that draws nothing where
  // it is clicked, so the move marks where it came to rest.
  const [moved, setMoved] = useState<{ index: number; flash: number } | null>(null);

  useEffect(() => {
    if (!moved) {
      return;
    }
    const timer = setTimeout(() => setMoved(null), ARRAY_ITEM_MOVED_FLASH_MS);
    return () => clearTimeout(timer);
  }, [moved]);

  // Another row on top of one still blank would leave two, so the last has to
  // hold something first. There is nothing to reorder in an array of one, and
  // nothing to throw away in an array of one blank row - whatever key an edit
  // left behind on it.
  const {
    maxIndex: maxArrayIndex,
    lastItemEmpty: lastArrayItemEmpty,
    hasItems,
  } = arrayExtent(settingsTree, keyPrefix, initialSettingsItems.length);

  const canReorder = !readOnly && maxArrayIndex > 0;

  // A row of an array of groups holds a form of its own and folds like any other
  // setting that does, so a long array reads as a list of rows rather than a run
  // of forms. A row of an array of values is one input already.
  const rowsFold = !itemsAreValues;

  const moveItem = (from: number, to: number) => {
    if (to >= 0 && to <= maxArrayIndex && to !== from) {
      onMoveArrayItem(keyPrefix, from, to);
      setMoved((current) => ({ index: to, flash: current ? 1 - current.flash : 0 }));
    }
  };

  // What a folded row is left showing of itself. Empty for a row with nothing
  // filled in yet, which is left to its number alone.
  const arrayRowSummaryValues = (elementKey: string, rowValue: InitialSettingsValue) =>
    rowSummaryValues(
      Array.isArray(rowValue) ? (rowValue as InitialSettings) : [],
      (child) =>
        // Editing reads the store, which is what the form is filled in from;
        // previewing reads the sample the mod declares, there being nothing in
        // the store for a mod that is not installed.
        readOnly
          ? declaredScalarValue(child.value)
          : modSettings[`${elementKey}.${child.key}`]
    );

  // Which row of this array a point of the page is in, or null for a point in
  // none of them. Climbing finds the row of this array holding the point, so a
  // row of a nested array belongs to that array rather than to this one. The row
  // under the buttons is not an element, so reaching it means the end of the
  // array. Climbed by hand rather than asked for as a selector: a key is the mod
  // author's, and one holding a quote would be a selector that does not parse.
  const arrayRowIndexAt = (target: EventTarget | null) => {
    for (
      let node = target instanceof Element ? target : null;
      node;
      node = node.parentElement
    ) {
      if (node.getAttribute('data-array-row') === keyPrefix) {
        const index = parseIntLax(node.getAttribute('data-array-row-index'));
        return index === -1 ? maxArrayIndex : index;
      }
    }

    return null;
  };

  // Carried by the pointer itself rather than by the browser's drag and drop,
  // which a webview host is free to take over for its own dropped files - and
  // where it does, an in-page drag never starts at all.
  //
  // The listeners hang off the document for as long as the drag lasts: a pointer
  // held down is reported wherever it goes, so a row carried off its list, or
  // off the window, is still the row being carried.
  const beginArrayItemDrag = (index: number, event: React.PointerEvent<HTMLElement>) => {
    // Only the primary button carries a row. Taking the press is the whole of
    // what the grip does with it, and it stops the selection the same press
    // would otherwise start dragging through the form.
    if (event.button !== 0) {
      return;
    }
    event.preventDefault();

    function detach() {
      endDragRef.current = null;
      document.removeEventListener('pointermove', move);
      document.removeEventListener('pointerup', drop);
      document.removeEventListener('pointercancel', abandon);
      document.removeEventListener('keydown', key);
    }

    function move(moveEvent: PointerEvent) {
      const current = dragRef.current;
      if (!current) {
        return;
      }

      // The grip in hand goes where the pointer goes, on or off the array: it
      // says what is in hand, which a drag with nowhere to land still is.
      if (dragBadgeRef.current) {
        dragBadgeRef.current.style.transform = arrayItemDragBadgeTransform(
          moveEvent.clientX,
          moveEvent.clientY,
          current.grab
        );
      }

      const row = arrayRowIndexAt(moveEvent.target);
      dragScroll.track(row === null ? null : moveEvent.clientY);

      // A drag carried off the array names the place it came from, so the drop
      // line goes with it and letting go out there leaves the array as it was.
      // Carried back on it names a place again: deactivated, not called off.
      const over = row ?? current.from;
      if (over !== current.over) {
        setDragTo({ ...current, over });
      }
    }

    function drop(upEvent: PointerEvent) {
      const current = dragRef.current;
      detach();
      setDragTo(null);
      dragScroll.end();

      if (current) {
        moveItem(current.from, current.over);
        setStaleHover({ row: arrayItemAt(upEvent.target) });
      }
    }

    // Nothing was landed on, so the row stays where it was and nothing is drawn
    // as under the pointer either.
    function abandon() {
      detach();
      setDragTo(null);
      dragScroll.end();
      setStaleHover((current) => current ?? { row: null });
    }

    function key(keyEvent: KeyboardEvent) {
      if (keyEvent.key === 'Escape') {
        abandon();
      }
    }

    // The grip's own box, so what comes up under the pointer is the grip where
    // it was rather than something appearing beside it.
    const grip = event.currentTarget.getBoundingClientRect();
    setDragTo({
      from: index,
      over: index,
      grip: { x: grip.left, y: grip.top, width: grip.width, height: grip.height },
      grab: { x: event.clientX - grip.left, y: event.clientY - grip.top },
    });
    setStaleHover(null);
    dragScroll.begin(event.currentTarget);

    endDragRef.current = abandon;
    document.addEventListener('pointermove', move);
    document.addEventListener('pointerup', drop);
    document.addEventListener('pointercancel', abandon);
    document.addEventListener('keydown', key);
  };

  const indexValues = [...Array(maxArrayIndex + 1).keys(), -1];

  return (
    <div
      // The move the hover was waiting for: the browser knows where the pointer
      // is again, so it can go back to saying so.
      onMouseMove={() => {
        if (staleHover) {
          setStaleHover(null);
        }
      }}
    >
      {drag && <ArrayItemDragCursor />}
      {drag &&
        createPortal(
          <ArrayItemDragBadge
            ref={dragBadgeRef}
            // Nothing to read: it draws the grip, which says nothing to a screen
            // reader either, for a drag only a pointer can be in the middle of.
            aria-hidden="true"
            data-testid="mod-setting-array-item-drag-badge"
            style={{
              width: drag.grip.width,
              height: drag.grip.height,
              transform: `translate(${drag.grip.x}px, ${drag.grip.y}px)`,
            }}
          >
            <FontAwesomeIcon icon={faGripVertical} />
          </ArrayItemDragBadge>,
          document.body
        )}
      <List
        itemLayout="vertical"
        split={false}
        dataSource={indexValues}
        renderItem={(index) => {
          const elementKey = `${keyPrefix}[${index}]`;

          // Which side of the row under the pointer the dragged one would come
          // to rest on: it takes this row's place, so it lands above a row it
          // reached from below and below one it reached from above.
          const dropEdge: DropEdge | undefined =
            drag && drag.over === index && drag.from !== index
              ? drag.from < index
                ? 'after'
                : 'before'
              : undefined;

          // The value the row is drawn from: the mod's template for a row being
          // edited, which the store fills in by key, and the row's own entry for
          // one being previewed, which carries values but nothing describing them.
          const rowValue =
            readOnly &&
            Array.isArray(initialSettingsItems[index]) &&
            Array.isArray(initialSettingsItems[0])
              ? mergeInitialSettingsMetadata(
                initialSettingsItems[0] as InitialSettings,
                initialSettingsItems[index] as InitialSettings
              )
              : initialSettingsItems[readOnly ? index : 0];

          const rowFolded = rowsFold && collapsedKeys.has(elementKey);
          const rowSummary = rowFolded
            ? arrayRowSummaryValues(elementKey, rowValue)
            : [];

          return (
            <SettingsListItem
              key={index}
              data-value-row={itemsAreValues && index !== -1 ? '' : undefined}
              // What names the row a drag is over. The list item carries it
              // rather than its content: the padding a row is set in is part of
              // the row, and a pointer crossing it is still headed somewhere.
              data-array-row={keyPrefix}
              data-array-row-index={index}
            >
              <div>
                {index === -1 ? (
                  <ArraySettingsFooter>
                    <Button
                      disabled={lastArrayItemEmpty}
                      onClick={() => onNewArrayItem(keyPrefix, maxArrayIndex + 1)}
                    >
                      {t('modDetails.settings.arrayItemAdd')}
                    </Button>
                    {/* Emptying throws away every value at once with no one row
                        to undo it from, so it is asked about first - as
                        reverting the whole form is. */}
                    {!readOnly && (
                      <PopconfirmModal
                        placement="topLeft"
                        disabled={!hasItems}
                        title={t('modDetails.settings.arrayItemsRemoveAllConfirm')}
                        okText={t('modDetails.settings.arrayItemsRemoveAll')}
                        cancelText={t('general.actions.cancel')}
                        onConfirm={() => onRemoveAllArrayItems(keyPrefix)}
                      >
                        <Button disabled={!hasItems}>
                          {t('modDetails.settings.arrayItemsRemoveAll')}
                        </Button>
                      </PopconfirmModal>
                    )}
                  </ArraySettingsFooter>
                ) : (
                  <ArraySettingsItemWrapper
                    data-array-item={elementKey}
                    data-array-item-reorderable={canReorder || undefined}
                    data-array-item-foldable={rowsFold || undefined}
                    data-array-item-dragging={drag?.from === index || undefined}
                    data-array-item-drop-edge={dropEdge}
                    data-array-item-moved={
                      moved?.index === index ? moved.flash : undefined
                    }
                    data-hover-held={drag || staleHover ? true : undefined}
                    data-hover-shown={staleHover?.row === elementKey || undefined}
                  >
                    {(!readOnly || rowsFold) && (
                      <ArraySettingsItemControls data-testid="mod-setting-array-item-controls">
                        {canReorder && (
                          <ArraySettingsItemDragHandle
                            aria-hidden="true"
                            data-array-drag-handle={elementKey}
                            data-testid="mod-setting-array-item-handle"
                            onPointerDown={(event) => beginArrayItemDrag(index, event)}
                          >
                            <FontAwesomeIcon icon={faGripVertical} />
                          </ArraySettingsItemDragHandle>
                        )}
                        {rowsFold && (
                          <CollapseCaret
                            place="arrayItem"
                            collapsed={rowFolded}
                            label={t(
                              rowFolded
                                ? 'modDetails.settings.arrayItemExpand'
                                : 'modDetails.settings.arrayItemCollapse',
                              { number: index + 1 }
                            )}
                            onToggle={() => onToggleCollapsed(elementKey)}
                          />
                        )}
                        {!readOnly && (
                          <DropdownModal
                            menu={{
                              items: [
                                ...(canReorder
                                  ? [
                                    {
                                      label: t('modDetails.settings.arrayItemMoveUp'),
                                      key: 'moveUp',
                                      disabled: index === 0,
                                      onClick: () => {
                                        moveItem(index, index - 1)
                                      },
                                    },
                                    {
                                      label: t('modDetails.settings.arrayItemMoveDown'),
                                      key: 'moveDown',
                                      disabled: index === maxArrayIndex,
                                      onClick: () => {
                                        moveItem(index, index + 1)
                                      },
                                    },
                                    { type: 'divider' as const, key: 'divider' },
                                  ]
                                  : []),
                                {
                                  label: t('modDetails.settings.arrayItemRemove'),
                                  key: 'remove',
                                  disabled: !hasItems,
                                  onClick: () => {
                                    onRemoveArrayItem(keyPrefix, index)
                                  },
                                },
                              ],
                            }}
                            trigger={['click']}
                          >
                            <ArraySettingsDropdownOptionsButton
                              data-array-item-menu=""
                              data-testid="mod-setting-array-item-menu"
                            >
                              <FontAwesomeIcon icon={faCaretDown} />
                            </ArraySettingsDropdownOptionsButton>
                          </DropdownModal>
                        )}
                      </ArraySettingsItemControls>
                    )}
                    <ArraySettingsItemContent data-testid="mod-setting-array-item-content">
                      {rowFolded ? (
                        <ArrayItemSummary
                          data-testid="mod-setting-array-item-summary"
                          onClick={foldingClickHandler(() =>
                            onToggleCollapsed(elementKey)
                          )}
                        >
                          <ArrayItemSummaryLabel>
                            {t('modDetails.settings.arrayItemNumber', {
                              number: index + 1,
                            })}
                          </ArrayItemSummaryLabel>
                          {rowSummary.length > 0 && (
                            <SettingSummary>{rowSummary.join(', ')}</SettingSummary>
                          )}
                        </ArrayItemSummary>
                      ) : (
                        <SingleSetting
                          initialSettingsValue={rowValue}
                          initialSettingItemExtra={initialSettingItemExtra}
                          settingKey={elementKey}
                        />
                      )}
                    </ArraySettingsItemContent>
                  </ArraySettingsItemWrapper>
                )}
              </div>
            </SettingsListItem>
          );
        }}
      />
    </div>
  );
}

interface ObjectSettingsProps {
  initialSettings: InitialSettings;
  keyPrefix?: string;
}

function ObjectSettings({ initialSettings, keyPrefix = '' }: ObjectSettingsProps) {
  const { t } = useTranslation();

  const settingsTree = useSettingsTree();
  const { density, collapsedKeys, onToggleCollapsed, onResetSetting } = settingsTree;

  // What a folded setting says about the form it folded away: how many rows an
  // array has, or how many settings a group opens.
  const foldedSummary = (value: InitialSettingsValue, settingKey: string) => {
    const descriptor = describeSetting(value);

    switch (descriptor.kind) {
      case SettingType.NestedObject:
        return t('modDetails.settings.foldedSettings', {
          count: descriptor.children.length,
        });

      case SettingType.NumberArray:
      case SettingType.StringArray:
      case SettingType.ObjectArray: {
        const { maxIndex, hasItems } = arrayExtent(
          settingsTree,
          settingKey,
          descriptor.value.length
        );
        return t('modDetails.settings.foldedItems', {
          count: hasItems ? maxIndex + 1 : 0,
        });
      }

      default:
        return undefined;
    }
  };

  return (
    <List
      itemLayout="vertical"
      split={false}
      dataSource={initialSettings}
      renderItem={(item) => {
        const settingKey = keyPrefix + item.key;
        const title = item.name || item.key;

        // A group or an array takes the state of anything under it, and its
        // reset puts the whole subtree back.
        const state = settingState(settingsTree, item.value, settingKey);

        // A compact row prints no description, carrying it on its title line
        // instead. A line left holding only the title is the plain string antd
        // renders it as, with no wrapper to hold nothing.
        const inlineDescription =
          density === 'compact' ? item.description : undefined;

        // A fold takes what the row holds out of the form, not out of the
        // settings: it comes back holding whatever it held, and what the row is
        // in is still marked in the gutter, so a fold hides no edit.
        const collapsible = isCollapsibleSetting(item.value);
        const collapsed = collapsible && collapsedKeys.has(settingKey);

        return (
          <SettingsListItem
            key={item.key}
            data-testid="mod-setting"
            data-setting-key={settingKey}
            data-setting-state={state}
            data-setting-collapsed={collapsed || undefined}
          >
            <SettingsListItemMeta
              title={
                state || inlineDescription || collapsible ? (
                  <SettingTitle
                    title={title}
                    state={state}
                    defaultLabel={defaultValueLabel(item)}
                    description={inlineDescription}
                    summary={collapsed ? foldedSummary(item.value, settingKey) : undefined}
                    collapsed={collapsed}
                    onToggleCollapse={
                      collapsible ? () => onToggleCollapsed(settingKey) : undefined
                    }
                    onReset={() => onResetSetting(settingKey)}
                  />
                ) : (
                  title
                )
              }
              description={density === 'compact' ? undefined : item.description}
            />
            {!collapsed && (
              <SingleSetting
                initialSettingsValue={item.value}
                initialSettingItemExtra={item}
                settingKey={settingKey}
              />
            )}
          </SettingsListItem>
        );
      }}
    />
  );
}

// ============================================================================
// Main View Component
// ============================================================================

/**
 * Whether the mod's declared defaults reach a key - the setting it names, or
 * anything under the group or the array it opens. A revert leaves exactly what
 * the mod declares, so this is also what says whether a row survives one.
 */
function defaultsReach(defaults: ModSettings, key: string): boolean {
  return Object.keys(defaults).some((defaultKey) => isKeyUnder(defaultKey, key));
}

export interface ModDetailsSettingsViewProps extends EditorViewModel {
  initialSettings: InitialSettings;

  // Read-only mode (for Website and the extension's preview views).
  readOnly?: boolean;
}

export function ModDetailsSettingsView({
  initialSettings,
  readOnly = false,
  mode,
  draft,
  canonicalDraft,
  canonicalSaved,
  arrayMaxIndex,
  yamlText,
  isDirty,
  anySettingModified,
  yamlAvailable,
  onChangeSetting,
  onAddArrayItem,
  onRemoveArrayItem,
  onRemoveAllArrayItems,
  onMoveArrayItem,
  onResetSetting,
  onSetYamlText,
  onToggleMode,
  onSave,
}: ModDetailsSettingsViewProps) {
  const { t } = useTranslation();

  // Fullscreen state: expand the settings to fill the whole window.
  const [isFullscreen, setIsFullscreen] = useState(false);

  // How tightly to draw the form. Kept across mods and visits, the way the
  // choice of editor is: it is how this user wants settings shown, not something
  // about the mod on screen.
  const [isCompact, toggleCompact] = usePersistedFlag(SETTINGS_DENSITY_STORAGE_KEY);

  // The preview has no toolbar to offer a way back out of compact, and it is the
  // one place the descriptions are the point, being what there is to read about
  // a mod that is not installed.
  const density: Density = !readOnly && isCompact ? 'compact' : 'comfortable';

  // Which groups and arrays are folded away. Held only for as long as the
  // settings are on screen: what one mod's form was left looking like says
  // nothing about the next mod's, and a remembered fold would quietly undo a
  // setting having been reached for again.
  const [collapsedKeys, setCollapsedKeys] = useState<ReadonlySet<string>>(
    () => new Set()
  );

  const toggleCollapsed = useCallback((key: string) => {
    setCollapsedKeys((current) => {
      const next = new Set(current);
      if (!next.delete(key)) {
        next.add(key);
      }
      return next;
    });
  }, []);

  // A key names a row of an array by the place it sits in, so an edit that moves
  // the rows has to carry the folds with them - otherwise a fold belongs to the
  // place rather than the row, and a row opens or closes under an edit that was
  // not about it. The rewrite is the one the draft's own keys go through, so the
  // two stay in step.
  const rewriteCollapsedKeys = useCallback(
    (rewrite: (key: string) => string | null) => {
      setCollapsedKeys((current) => {
        const next = new Set<string>();
        let changed = false;

        for (const key of current) {
          const rewritten = rewrite(key);
          if (rewritten === null) {
            changed = true;
          } else {
            changed ||= rewritten !== key;
            next.add(rewritten);
          }
        }

        return changed ? next : current;
      });
    },
    []
  );

  const moveArrayItem = useCallback(
    (prefix: string, from: number, to: number) => {
      onMoveArrayItem(prefix, from, to);
      rewriteCollapsedKeys((key) => rewriteKeyAfterMove(key, prefix, from, to));
    },
    [onMoveArrayItem, rewriteCollapsedKeys]
  );

  const removeArrayItem = useCallback(
    (prefix: string, index: number) => {
      onRemoveArrayItem(prefix, index);
      // The row is gone, and with it whatever was folded inside it. The rows
      // after it close the gap, so their folds come down a place with them.
      rewriteCollapsedKeys((key) =>
        indexAtPrefix(key, prefix) === index
          ? null
          : rewriteKeyAfterRemove(key, prefix, index)
      );
    },
    [onRemoveArrayItem, rewriteCollapsedKeys]
  );

  const removeAllArrayItems = useCallback(
    (prefix: string) => {
      onRemoveAllArrayItems(prefix);
      // Nothing is left under the array to be folded. The array's own fold is
      // not under it and stays: emptying it is not opening it out.
      rewriteCollapsedKeys((key) =>
        indexAtPrefix(key, prefix) === null ? key : null
      );
    },
    [onRemoveAllArrayItems, rewriteCollapsedKeys]
  );

  // The same defaults the editor reverts to, which is what says how much of a
  // subtree is left standing after a revert.
  const settingDefaults = useMemo(
    () => flattenAllDefaults(initialSettings),
    [initialSettings]
  );

  const resetSetting = useCallback(
    (keyPrefix: string) => {
      onResetSetting(keyPrefix);
      // A revert puts back an array of the declared length however many rows it
      // held, so a fold on a row the declaration does not reach is a fold on a
      // row that is gone. Everything it does reach is left as it is: reverting a
      // row's values says nothing about whether the row is folded.
      rewriteCollapsedKeys((key) =>
        !isKeyUnder(key, keyPrefix) || defaultsReach(settingDefaults, key)
          ? key
          : null
      );
    },
    [onResetSetting, rewriteCollapsedKeys, settingDefaults]
  );

  // Whether the text editor wraps a line too long for it. Kept the way the
  // density is, being the same kind of choice about the same settings.
  const [isWordWrap, toggleWordWrap] = usePersistedFlag(SETTINGS_WORD_WRAP_STORAGE_KEY);

  const showYamlEditor = mode === 'yaml' && !!MonacoYamlEditor;

  // Mark the body while fullscreen so app-level fixed overlays (e.g. the
  // "Create a new mod" button) can hide themselves behind the settings.
  useEffect(() => {
    if (readOnly) {
      return;
    }

    const className = 'windhawk-settings-fullscreen';
    document.body.classList.toggle(className, isFullscreen);
    return () => document.body.classList.remove(className);
  }, [isFullscreen, readOnly]);

  // Keyboard shortcut (F11) to toggle fullscreen. Not available in preview mode.
  useKeyboardShortcut(
    !readOnly,
    (e) => e.key === 'F11',
    () => setIsFullscreen((value) => !value)
  );

  // Keyboard shortcut (Ctrl+S) to save.
  useKeyboardShortcut(!readOnly, (e) => e.key === 's' && e.ctrlKey, onSave);

  // Keyboard shortcut (Alt+Z) to toggle word wrap, the way an editor binds it.
  // Only while the text editor is on screen: it is what wraps.
  useKeyboardShortcut(
    showYamlEditor,
    (e) => e.key.toLowerCase() === 'z' && e.altKey && !e.ctrlKey && !e.shiftKey,
    toggleWordWrap
  );

  // What the whole tree is drawn from, put up once for all of it.
  const settingsTree = useMemo<SettingsTreeProps>(
    () => ({
      modSettings: draft,
      canonicalDraft,
      canonicalSaved,
      density,
      onSettingChanged: onChangeSetting,
      arrayItemMaxIndex: arrayMaxIndex,
      // The edits that take rows away or move them around, each of them
      // carrying what is folded along with them.
      onRemoveArrayItem: removeArrayItem,
      onRemoveAllArrayItems: removeAllArrayItems,
      onMoveArrayItem: moveArrayItem,
      onNewArrayItem: onAddArrayItem,
      onResetSetting: resetSetting,
      collapsedKeys,
      onToggleCollapsed: toggleCollapsed,
      readOnly,
    }),
    [
      draft,
      canonicalDraft,
      canonicalSaved,
      density,
      onChangeSetting,
      arrayMaxIndex,
      removeArrayItem,
      removeAllArrayItems,
      moveArrayItem,
      onAddArrayItem,
      resetSetting,
      collapsedKeys,
      toggleCollapsed,
      readOnly,
    ]
  );

  const fullscreenLabel = isFullscreen
    ? t('modDetails.settings.collapse')
    : t('modDetails.settings.expand');

  return (
    <SettingsForm
      $fullscreen={isFullscreen}
      onSubmit={(e) => {
        e.preventDefault();
        onSave();
      }}
    >
      <SaveSettingsCard $fullscreen={isFullscreen} bordered={false} size="small">
        <SaveSettingsHeader>
          {readOnly ? (
            <SaveSettingsHeaderMain>
              <Alert
                type="info"
                message={t('modDetails.settings.readOnlyPreview')}
              />
            </SaveSettingsHeaderMain>
          ) : (
            <>
              <ToolbarGroup>
                <Button
                  type="primary"
                  htmlType="submit"
                  title="Ctrl+S"
                  aria-keyshortcuts="Control+S"
                  disabled={!isDirty}
                  data-testid="mod-settings-save"
                >
                  {t('modDetails.settings.saveButton')}
                </Button>
                {/* Rare next to saving and far-reaching, so an icon beside the
                    button it undoes rather than one competing with it. It keeps
                    its place whether or not there is anything to revert. */}
                <PopconfirmModal
                  placement="bottomLeft"
                  disabled={!anySettingModified}
                  title={t('modDetails.settings.resetAllConfirm')}
                  okText={t('modDetails.settings.resetAll')}
                  cancelText={t('general.actions.cancel')}
                  onConfirm={() => resetSetting('')}
                >
                  <ToolbarButton
                    disabled={!anySettingModified}
                    title={t('modDetails.settings.resetAll')}
                    aria-label={t('modDetails.settings.resetAll')}
                    data-testid="mod-settings-reset-all"
                  >
                    <FontAwesomeIcon icon={faRotateLeft} />
                  </ToolbarButton>
                </PopconfirmModal>
              </ToolbarGroup>
              <ViewControlsGroup>
                {/* Which editor is on screen is a state, not an action, so both
                    are shown with the current one marked - a single button
                    naming the other mode reads as the mode it is in. The empty
                    title says not to draw the tooltip antd would take from the
                    label, which repeats what is already read. */}
                {MonacoYamlEditor && yamlAvailable && (
                  <ModeSegmented
                    data-testid="mod-settings-mode-toggle"
                    value={mode}
                    options={[
                      { value: 'ui', label: t('modDetails.settings.uiMode'), title: '' },
                      { value: 'yaml', label: t('modDetails.settings.yamlMode'), title: '' },
                    ]}
                    onChange={(value) => {
                      if (value !== mode) {
                        onToggleMode();
                      }
                    }}
                  />
                )}
                {/* One slot, holding whichever toggle is about the editor on
                    screen. Neither means anything to the other editor, and the
                    slot is filled either way, so switching mode moves nothing
                    else on the strip. */}
                {showYamlEditor ? (
                  <ViewToggleButton
                    $pressed={isWordWrap}
                    aria-pressed={isWordWrap}
                    title={`${t('modDetails.settings.wordWrap')} (Alt+Z)`}
                    aria-label={t('modDetails.settings.wordWrap')}
                    aria-keyshortcuts="Alt+Z"
                    data-testid="mod-settings-word-wrap-toggle"
                    onClick={toggleWordWrap}
                  >
                    <FontAwesomeIcon icon={faTextWidth} />
                  </ViewToggleButton>
                ) : (
                  <ViewToggleButton
                    $pressed={density === 'compact'}
                    aria-pressed={density === 'compact'}
                    title={t('modDetails.settings.compactView')}
                    aria-label={t('modDetails.settings.compactView')}
                    data-testid="mod-settings-density-toggle"
                    onClick={toggleCompact}
                  >
                    <FontAwesomeIcon icon={faTableList} />
                  </ViewToggleButton>
                )}
                <ToolbarButton
                  title={`${fullscreenLabel} (F11)`}
                  aria-label={fullscreenLabel}
                  aria-keyshortcuts="F11"
                  onClick={() => setIsFullscreen((value) => !value)}
                >
                  <FontAwesomeIcon icon={isFullscreen ? faCompress : faExpand} />
                </ToolbarButton>
              </ViewControlsGroup>
            </>
          )}
        </SaveSettingsHeader>
      </SaveSettingsCard>
      {showYamlEditor ? (
        <Suspense fallback={null}>
          <MonacoYamlEditor
            yamlText={yamlText}
            onYamlTextChange={onSetYamlText}
            fullscreen={isFullscreen}
            wordWrap={isWordWrap}
          />
        </Suspense>
      ) : (
        // The size antd draws a control at, asked for once around the form
        // rather than at each input. A control drawn at one size whatever the
        // form is in - the carets, the reset - says so itself and is left alone.
        //
        // A nested card reads the size too and takes antd's smaller body padding
        // with it at the compact density, but the form's own padding overrides
        // that, so the card is drawn with the gap named in DENSITY either way.
        <ConfigProvider componentSize={DENSITY[density].controlSize}>
          <SettingsTreeContext.Provider value={settingsTree}>
            <SettingsWrapper $density={density}>
              <ObjectSettings initialSettings={initialSettings} />
            </SettingsWrapper>
          </SettingsTreeContext.Provider>
        </ConfigProvider>
      )}
    </SettingsForm>
  );
}
