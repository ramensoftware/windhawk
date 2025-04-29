// eslint-disable-next-line @typescript-eslint/ban-ts-comment
// @ts-nocheck: ignore TS errors due to lack of types for react-diff-view and refractor

import {
  faArrowsAltV,
  faLongArrowAltDown,
  faLongArrowAltUp,
} from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Button, Switch } from 'antd';
import { useCallback, useMemo, useState } from 'react';
import {
  Decoration,
  Diff,
  getCollapsedLinesCountBetween,
  Hunk,
  markEdits,
  parseDiff,
  tokenize,
  useMinCollapsedLines,
  useSourceExpansion,
} from 'react-diff-view';
import { useTranslation } from 'react-i18next';
import refractor from 'refractor';
import styled from 'styled-components';
import { diffLines, formatLines } from 'unidiff';

const ConfigurationWrapper = styled.div`
  margin-bottom: 20px;

  > span {
    vertical-align: middle;
  }

  > button {
    margin-left: 10px;
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

// https://codesandbox.io/s/react-diff-view-mark-edits-demo-8ndcl

const diffTokenize = (hunks, oldSource) => {
  if (!hunks) {
    return undefined;
  }

  const options = {
    highlight: true,
    language: 'cpp',
    refractor,
    oldSource,
    enhancers: [markEdits(hunks, { type: 'block' })],
  };

  try {
    return tokenize(hunks, options);
  } catch {
    return undefined;
  }
};

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

  const tokens = diffTokenize(hunksWithMinLinesCollapsed, oldSource);

  const renderHunk = (children, hunk, i, hunks) => {
    const previousElement = children[children.length - 1];
    const decorationElement = oldSource ? (
      <UnfoldCollapsed
        key={'decoration-' + hunk.content}
        previousHunk={previousElement && previousElement.props.hunk}
        currentHunk={hunk}
        linesCount={linesCount}
        onExpand={expandRange}
      />
    ) : (
      <Decoration key={'decoration-' + hunk.content} hunk={hunk}>
        {null}
        {hunk.content}
      </Decoration>
    );
    children.push(decorationElement);

    const hunkElement = <Hunk key={'hunk-' + hunk.content} hunk={hunk} />;
    children.push(hunkElement);

    if (i === hunks.length - 1 && oldSource) {
      const unfoldTailElement = (
        <UnfoldCollapsed
          key="decoration-tail"
          previousHunk={hunk}
          linesCount={linesCount}
          onExpand={expandRange}
        />
      );
      children.push(unfoldTailElement);
    }

    return children;
  };

  return (
    <>
      <ConfigurationWrapper>
        <span>{t('modDetails.changes.splitView')}</span>
        <Switch
          checked={splitView}
          onChange={(checked) => setSplitView(checked)}
        />
      </ConfigurationWrapper>
      <Diff
        optimizeSelection
        viewType={splitView ? 'split' : 'unified'}
        diffType={type}
        hunks={hunksWithMinLinesCollapsed}
        oldSource={oldSource}
        tokens={tokens}
      >
        {(hunks) => hunks.reduce(renderHunk, [])}
      </Diff>
    </>
  );
}

export default ModDetailsSource;
