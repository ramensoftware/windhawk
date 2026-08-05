// eslint-disable-next-line @typescript-eslint/ban-ts-comment
// @ts-nocheck: ignore TS errors due to lack of types for react-diff-view and refractor

import {
  faArrowsAltV,
  faLongArrowAltDown,
  faLongArrowAltUp,
} from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Button, ConfigProvider, Switch } from 'antd';
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Decoration,
  Diff,
  getCollapsedLinesCountBetween,
  Hunk,
  parseDiff,
  useMinCollapsedLines,
  useSourceExpansion,
} from 'react-diff-view';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { diffLines, formatLines } from 'unidiff';
import { sourceDiffTokens } from './sourceDiffTokens';

const ConfigurationWrapper = styled.div`
  margin-bottom: 20px;

  > span {
    vertical-align: middle;
  }

  > button {
    margin-inline-start: 10px;
  }
`;

const DiffWrapper = styled.div`
  direction: ltr;

  // A diff says what changed through the row tints alone, and forced colors
  // (Windows High Contrast) flattens every one of them to the page background,
  // leaving added and removed lines indistinguishable. There is no system color
  // pair that carries insert-vs-delete, so keep the themed palette here; both
  // themes already meet contrast on their own.
  @media (forced-colors: active) {
    forced-color-adjust: none;
  }
`;

const UnfoldButton = styled(Button)`
  width: 100%;
  border-radius: 0;
`;

// https://github.com/otakustay/react-diff-view/blob/f9e5f9f248f331598e5c9e7839fccb211efe43c2/site/components/DiffView/Unfold.js

const ICON_TYPE_MAPPING = {
  up: faLongArrowAltUp,
  down: faLongArrowAltDown,
  none: faArrowsAltV,
};

const Unfold = ({ start, end, direction, onExpand, ...props }) => {
  const { t } = useTranslation();

  const expand = useCallback(
    () => onExpand(start, end),
    [onExpand, start, end]
  );

  const iconType = ICON_TYPE_MAPPING[direction];
  const lines = end - start;

  return (
    <Decoration {...props}>
      <UnfoldButton onClick={expand}>
        <FontAwesomeIcon icon={iconType} />
        &nbsp;{t('modDetails.changes.expandLines', { count: lines })}
      </UnfoldButton>
    </Decoration>
  );
};

// https://github.com/otakustay/react-diff-view/blob/f9e5f9f248f331598e5c9e7839fccb211efe43c2/site/components/DiffView/UnfoldCollapsed.js

const UnfoldCollapsed = ({
  previousHunk,
  currentHunk,
  linesCount,
  onExpand,
}) => {
  if (!currentHunk) {
    const nextStart = previousHunk.oldStart + previousHunk.oldLines;
    const collapsedLines = linesCount - nextStart + 1;

    if (collapsedLines <= 0) {
      return null;
    }

    return (
      <>
        {collapsedLines > 10 && (
          <Unfold
            direction="down"
            start={nextStart}
            end={nextStart + 10}
            onExpand={onExpand}
          />
        )}
        <Unfold
          direction="none"
          start={nextStart}
          end={linesCount + 1}
          onExpand={onExpand}
        />
      </>
    );
  }

  const collapsedLines = getCollapsedLinesCountBetween(
    previousHunk,
    currentHunk
  );

  if (!previousHunk) {
    if (!collapsedLines) {
      return null;
    }

    const start = Math.max(currentHunk.oldStart - 10, 1);

    return (
      <>
        <Unfold
          direction="none"
          start={1}
          end={currentHunk.oldStart}
          onExpand={onExpand}
        />
        {collapsedLines > 10 && (
          <Unfold
            direction="up"
            start={start}
            end={currentHunk.oldStart}
            onExpand={onExpand}
          />
        )}
      </>
    );
  }

  const collapsedStart = previousHunk.oldStart + previousHunk.oldLines;
  const collapsedEnd = currentHunk.oldStart;

  if (collapsedLines < 10) {
    return (
      <Unfold
        direction="none"
        start={collapsedStart}
        end={collapsedEnd}
        onExpand={onExpand}
      />
    );
  }

  return (
    <>
      <Unfold
        direction="down"
        start={collapsedStart}
        end={collapsedStart + 10}
        onExpand={onExpand}
      />
      <Unfold
        direction="none"
        start={collapsedStart}
        end={collapsedEnd}
        onExpand={onExpand}
      />
      <Unfold
        direction="up"
        start={collapsedEnd - 10}
        end={collapsedEnd}
        onExpand={onExpand}
      />
    </>
  );
};

// Neither the library's Hunk nor the decoration above is memoized on its own, so
// without this any rerender reconciles every row and every button in the file
// rather than the handful that changed.
//
// This no longer covers unfolding, which remounts the diff outright for the
// reason the key on <Diff> gives, and so rebuilds every hunk anyway. It still
// covers a rerender that leaves that key alone, a language or theme change being
// the ordinary one.
const MemoizedHunk = memo(Hunk);
const MemoizedUnfoldCollapsed = memo(UnfoldCollapsed);

interface Props {
  oldSource: string;
  newSource: string;
}

function ModDetailsSource(props: Props) {
  const { t } = useTranslation();

  const { oldSource, newSource } = props;

  const [splitView, setSplitView] = useState(true);

  const { type, hunks } = useMemo(() => {
    const diffText = formatLines(diffLines(oldSource, newSource), {
      context: 3,
    });
    const [{ type, hunks }] = parseDiff(diffText, { nearbySequences: 'zip' });
    return { type, hunks };
  }, [newSource, oldSource]);

  // https://github.com/otakustay/react-diff-view/blob/b9213164497211ef45393e5a57ed5866a5f27b2e/site/components/DiffView/index.js

  const [hunksWithSourceExpanded, expandRange] = useSourceExpansion(
    hunks,
    oldSource
  );
  const hunksWithMinLinesCollapsed = useMinCollapsedLines(
    0,
    hunksWithSourceExpanded,
    oldSource
  );
  const linesCount = oldSource ? oldSource.split('\n').length : 0;

  // useSourceExpansion builds its expander fresh on every render, which would
  // land on every gap's buttons as a changed prop and undo their memo. Pin it to
  // one identity that reads the current one.
  const expandRangeRef = useRef(expandRange);
  useEffect(() => {
    expandRangeRef.current = expandRange;
  });
  // Bumped on every expansion so the diff below can be keyed on it. The value
  // itself means nothing; that it changes is the point.
  const [expandCount, setExpandCount] = useState(0);
  const onExpand = useCallback((start, end) => {
    expandRangeRef.current(start, end);
    setExpandCount((n) => n + 1);
  }, []);

  // Tokenizing is by far the most expensive thing here, and it is over the whole
  // of both sources with marks that come from the diff, so how much of the file
  // is currently unfolded does not enter into it: these are the hunks the diff
  // parsed, not the expanded ones.
  //
  // Keying on those rather than on the expanded hunks is what keeps unfolding
  // affordable. The remount below rebuilds the rows either way, but it reuses
  // this result; retokenizing the file on top of that would be far and away the
  // larger cost of the two.
  const tokens = useMemo(
    () => sourceDiffTokens(hunks, oldSource, newSource),
    [hunks, newSource, oldSource]
  );

  const renderHunk = (children, hunk, i, hunks) => {
    const previousElement = children[children.length - 1];
    const decorationElement = oldSource ? (
      <MemoizedUnfoldCollapsed
        key={'decoration-' + hunk.content}
        previousHunk={previousElement && previousElement.props.hunk}
        currentHunk={hunk}
        linesCount={linesCount}
        onExpand={onExpand}
      />
    ) : (
      <Decoration key={'decoration-' + hunk.content} hunk={hunk}>
        {null}
        {hunk.content}
      </Decoration>
    );
    children.push(decorationElement);

    const hunkElement = (
      <MemoizedHunk key={'hunk-' + hunk.content} hunk={hunk} />
    );
    children.push(hunkElement);

    if (i === hunks.length - 1 && oldSource) {
      const unfoldTailElement = (
        <MemoizedUnfoldCollapsed
          key="decoration-tail"
          previousHunk={hunk}
          linesCount={linesCount}
          onExpand={onExpand}
        />
      );
      children.push(unfoldTailElement);
    }

    return children;
  };

  const viewType = splitView ? 'split' : 'unified';

  return (
    <ConfigProvider direction="ltr">
      <ConfigurationWrapper>
        <span>{t('modDetails.changes.splitView')}</span>
        <Switch
          checked={splitView}
          onChange={(checked) => setSplitView(checked)}
        />
      </ConfigurationWrapper>
      <DiffWrapper>
        <Diff
          // Rebuilt from scratch on every change, which costs a full render but
          // is the only update shape Chromium can apply cheaply once
          // accessibility is on).
          //
          // Chromium turns a DOM change into platform accessibility events that
          // the browser process fires one at a time, synchronously, from the
          // thread pumping its UI message loop, and the count is not the number
          // of nodes changed but the number of nodes FOLLOWING the change in
          // the live tree. So an insertion into the middle of a big subtree
          // reserializes everything after it, while replacing the subtree
          // wholesale costs a handful of events because nothing follows it.
          // Measured against an 8000 element table: 7012 events to insert one
          // row in the middle, 7 to append the same row at the end, 4 to swap
          // the whole table. Not a table quirk, plain divs behave the same.
          //
          // Unfolding is the flow that suffers. It inserts a few hundred nodes
          // into the middle of a table of tens of thousands, which is 618 nodes
          // touched but ~36k accessibility events, and on the large mod fixture
          // that froze the whole browser (not just this view) for 59 seconds a
          // click. Keying the diff on the expansion count makes React drop the
          // table and mount the replacement detached instead, which took the
          // same four clicks to 140 events and 1.1s.
          //
          // The cost is a full rerender per unfold, 75ms to 685ms on that
          // fixture, since every hunk is rebuilt rather than the one that
          // changed. That is the trade being made deliberately: the memoization
          // above still keeps the render itself as cheap as it can be, but it
          // cannot make an in-place insertion cheap for Chromium.
          //
          // Rebuilding only from the changed hunk downwards looks like it
          // should win here, and it does keep the events away (a tail has
          // nothing after it either, 24 events against 6671 for an in-place
          // insert). It is not worth the keying it costs: the gap being opened
          // is usually near the top of what is still folded, so the tail is
          // nearly the whole file anyway. Measured unfolding top to bottom it
          // came out at a 690ms median against 685ms for rebuilding everything.
          //
          // Getting a genuinely cheap unfold means not holding the whole file
          // in the DOM, because both costs, Chromium's and React's, scale with
          // how much of it sits below the change. That is windowing, which
          // react-diff-view 3.3.3 does not do, so it means replacing how this
          // renders rather than keying it differently.
          //
          // viewType is in the key for the same reason, though it matters much
          // less: a toggle rebuilds every row either way, and keying it only
          // moves 5069 events to 1.
          //
          // None of this shows up without an accessibility client running,
          // which is why a clean machine looks fine.
          // `--force-renderer-accessibility` reproduces it on demand.
          key={`${viewType}-${expandCount}`}
          optimizeSelection
          viewType={viewType}
          diffType={type}
          hunks={hunksWithMinLinesCollapsed}
          oldSource={oldSource}
          tokens={tokens}
        >
          {(hunks) => hunks.reduce(renderHunk, [])}
        </Diff>
      </DiffWrapper>
    </ConfigProvider>
  );
}

export default ModDetailsSource;
