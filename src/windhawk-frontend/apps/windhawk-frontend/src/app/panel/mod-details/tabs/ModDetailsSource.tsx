import { faCheck, faCopy } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Button, Switch, Tooltip } from 'antd';
import 'prism-themes/themes/prism-vsc-dark-plus.css';
import Prism from 'prismjs';
import 'prismjs/components/prism-c';
import 'prismjs/components/prism-cpp';
import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { DropdownModal } from '@app/components/InputWithContextMenu';
import { useCopyToClipboard } from '@app/panel/shared/useCopyToClipboard';
import { copyTextToClipboard } from '@app/utils';
import { findCommentBlockBody } from '../modSourceBlocks';

const SyntaxHighlighterWrapper = styled.div`
  direction: ltr;

  pre {
    font-size: 13px;
    line-height: 1.5;
    background-color: var(--whui-background-color);
    padding: 12px;
    border-radius: 2px;
    overflow: auto;
  }

  code {
    color: var(--whui-editor-fg);
    background-color: transparent;
    tab-size: 4;
  }
`;

// Zero height, so the button hangs over the code's top corner without pushing
// it down, and stays in view as the code scrolls past. It sits outside the
// pre, which is a scroll container of its own and would pin it there instead.
const CopyButtonAnchor = styled.div`
  position: sticky;
  top: 0;
  z-index: 1;
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

  const highlightedHtml = useMemo(
    () => Prism.highlight(currentSource, Prism.languages['cpp'], 'cpp'),
    [currentSource]
  );

  const { copied, copy } = useCopyToClipboard();
  const copyLabel = copied
    ? t('modDetails.code.copied')
    : t('modDetails.code.copy');

  return (
    <>
      <ConfigurationWrapper>
        <span>{t('modDetails.code.collapseExtra')}</span>
        <Switch
          checked={isCollapsed}
          onChange={(checked) => setIsCollapsed(checked)}
        />
      </ConfigurationWrapper>
      <DropdownModal
        // Rewriting the highlighted HTML of an element that is already in the
        // accessibility tree freezes Chromium (fine in Firefox). The rewrite
        // turns into platform accessibility events that the browser process
        // fires one at a time, synchronously, from the same thread that pumps
        // its UI message loop, so the whole browser stops responding for as
        // long as it takes. Measured over 4000 elements: ~16k events and a 29
        // second stall, against 5 events and no visible pause for the same
        // content built detached and swapped in. The renderer costs the same
        // either way (~30ms), so the swap is free.
        //
        // What Chromium charges for is the nodes following a change in the live
        // tree, not the nodes changed, so replacing a subtree outright is the
        // one cheap update shape. ModDetailsSourceDiff has the same problem and
        // the same fix, with the measurements that pin the rule down.
        //
        // Keying on currentSource is what buys the swap: React mounts the new
        // subtree detached and replaces the old one in a single insertion, and
        // the accessibility tree sees one subtree replacing another instead of
        // thousands of in-place edits.
        //
        // It only bites when accessibility is enabled, which needs a screen
        // reader or any other UIA client running. That is why a clean machine
        // never shows it. `--force-renderer-accessibility` reproduces it on
        // demand.
        key={currentSource}
        menu={{
          items: [
            {
              label: t('general.contextMenu.copy'),
              key: 'copy',
              onClick: () => {
                // navigator.clipboard.writeText is forbidden in VSCode webviews.
                const selection = window.getSelection();
                if (selection && selection.type === 'Range') {
                  document.execCommand('copy');
                } else {
                  copyTextToClipboard(source);
                }
              },
            },
          ],
        }}
        trigger={['contextMenu']}
      >
        <SyntaxHighlighterWrapper>
          <CopyButtonAnchor>
            <Tooltip title={copyLabel} placement="left">
              <Button
                size="small"
                aria-label={copyLabel}
                icon={<FontAwesomeIcon icon={copied ? faCheck : faCopy} />}
                // The whole source, whether or not the view collapses it.
                onClick={() => copy(source)}
              />
            </Tooltip>
          </CopyButtonAnchor>
          <pre>
            <code dangerouslySetInnerHTML={{ __html: highlightedHtml }} />
          </pre>
        </SyntaxHighlighterWrapper>
      </DropdownModal>
    </>
  );
}

export default ModDetailsSource;
