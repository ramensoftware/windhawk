import { faCheck, faCopy } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Button, Switch, Tooltip } from 'antd';
import { lazy, Suspense, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { DropdownModal } from '@app/components/InputWithContextMenu';
import { useCopyToClipboard } from '@app/panel/shared/useCopyToClipboard';
import { copyTextToClipboard } from '@app/utils';
import { findCommentBlockBody } from '../modSourceBlocks';
import type { SourceCodeViewHandle } from './SourceCodeView';

declare const WEBPACK_IS_VSCODE: boolean;

// The editor comes with the view, and no other screen needs it. The split is
// here rather than at the tab: a module a split-off chunk shares with the main
// bundle stays there unhoisted, and the tab shares far more (antd, the app's
// components) than the view does.
const SourceCodeView = lazy(() => import('./SourceCodeView'));

const SourceWrapper = styled.div`
  direction: ltr;

  // What the search panel keeps clear for the copy button: its width and the
  // gap before it.
  --source-search-inset: 32px;
`;

// Zero height, so the button hangs over the code's top corner without pushing
// it down, and stays in view as the code scrolls past - over the search panel,
// which sticks to the same edge. It sits outside the code view, whose
// horizontal scroller would pin it there instead.
const CopyButtonAnchor = styled.div`
  position: sticky;
  top: 0;
  z-index: 2;
  height: 0;
  display: flex;
  align-items: flex-start;
  justify-content: flex-end;
  padding-inline-end: 8px;

  > button {
    margin-top: 8px;
  }
`;

const ConfigurationWrapper = styled.div`
  margin-bottom: 20px;

  > span {
    vertical-align: middle;
  }

  > button {
    margin-inline-start: 10px;
  }
`;

// The block comes off the same scan the readme and the settings are read with,
// so the view collapses what the app reads, and a mod whose comment never closes
// costs one pass either way.
function collapseBlock(source: string, name: string) {
  const body = findCommentBlockBody(source, name);
  if (body === null) {
    return source;
  }

  // A body carrying a terminator of its own runs past where the comment ends,
  // so an ellipsis over it would fold code into the comment.
  if (source.slice(body.start, body.end).includes('*/')) {
    return source;
  }

  return source.slice(0, body.start) + '...' + source.slice(body.end);
}

function collapseSource(source: string) {
  return collapseBlock(
    collapseBlock(source, 'WindhawkModReadme'),
    'WindhawkModSettings'
  );
}

interface Props {
  source: string;
}

function ModDetailsSource({ source }: Props) {
  const { t } = useTranslation();

  const [isCollapsed, setIsCollapsed] = useState(true);
  const collapsedSource = useMemo(() => collapseSource(source), [source]);
  const currentSource = isCollapsed ? collapsedSource : source;

  const sourceViewRef = useRef<SourceCodeViewHandle>(null);

  const { copied, copy } = useCopyToClipboard();
  const copyLabel = copied
    ? t('modDetails.code.copied')
    : t('modDetails.code.copy');

  const sourceView = (
    <SourceWrapper>
      <CopyButtonAnchor>
        <Tooltip
          title={copyLabel}
          placement="bottomRight"
          arrow={{ pointAtCenter: true }}
        >
          <Button
            size="small"
            aria-label={copyLabel}
            icon={<FontAwesomeIcon icon={copied ? faCheck : faCopy} />}
            // The whole source, whether or not the view collapses it.
            onClick={() => copy(source)}
          />
        </Tooltip>
      </CopyButtonAnchor>
      <Suspense fallback={null}>
        <SourceCodeView ref={sourceViewRef} source={currentSource} />
      </Suspense>
    </SourceWrapper>
  );

  return (
    <>
      <ConfigurationWrapper>
        <span>{t('modDetails.code.collapseExtra')}</span>
        <Switch
          checked={isCollapsed}
          onChange={(checked) => setIsCollapsed(checked)}
        />
      </ConfigurationWrapper>
      {/* The VSCode webview has no context menu of its own. Elsewhere the
          native one copies through CodeMirror, which reads the whole selection. */}
      {WEBPACK_IS_VSCODE ? (
        <DropdownModal
          menu={{
            items: [
              {
                label: t('general.contextMenu.copy'),
                key: 'copy',
                onClick: () => {
                  // The view's own selection rather than the document's, which
                  // holds only the lines that are drawn. navigator.clipboard is
                  // forbidden in VSCode webviews.
                  const selected = sourceViewRef.current?.selectedText();
                  if (selected) {
                    copyTextToClipboard(selected);
                    sourceViewRef.current?.focus();
                  } else {
                    copyTextToClipboard(source);
                  }
                },
              },
            ],
          }}
          trigger={['contextMenu']}
        >
          {sourceView}
        </DropdownModal>
      ) : (
        sourceView
      )}
    </>
  );
}

export default ModDetailsSource;
