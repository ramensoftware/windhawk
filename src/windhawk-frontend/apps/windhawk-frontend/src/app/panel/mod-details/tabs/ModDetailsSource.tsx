import { Switch } from 'antd';
import 'prism-themes/themes/prism-vsc-dark-plus.css';
import Prism from 'prismjs';
import 'prismjs/components/prism-c';
import 'prismjs/components/prism-cpp';
import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { DropdownModal } from '@app/components/InputWithContextMenu';

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

const ConfigurationWrapper = styled.div`
  margin-bottom: 20px;

  > span {
    vertical-align: middle;
  }

  > button {
    margin-inline-start: 10px;
  }
`;

function collapseSource(source: string) {
  return source
    .replace(
      /^(\/\/[ \t]+==WindhawkModReadme==[ \t]*$\s*\/\*)(\s*[\s\S]+?\s*)(\*\/\s*^\/\/[ \t]+==\/WindhawkModReadme==[ \t]*)$/m,
      (match, p1, p2, p3) => {
        if ((p2 as string).includes('*/')) {
          return p1 + p2 + p3;
        }
        return p1 + '...' + p3;
      }
    )
    .replace(
      /^(\/\/[ \t]+==WindhawkModSettings==[ \t]*$\s*\/\*)(\s*[\s\S]+?\s*)(\*\/\s*^\/\/[ \t]+==\/WindhawkModSettings==[ \t]*)$/m,
      (match, p1, p2, p3) => {
        if ((p2 as string).includes('*/')) {
          return p1 + p2 + p3;
        }
        return p1 + '...' + p3;
      }
    );
}

// https://stackoverflow.com/a/30810322
function fallbackCopyTextToClipboard(text: string) {
  const textArea = document.createElement('textarea');
  textArea.value = text;

  // Avoid scrolling to bottom.
  textArea.style.top = '0';
  textArea.style.insetInlineStart = '0';
  textArea.style.position = 'fixed';

  document.body.appendChild(textArea);
  textArea.focus();
  textArea.select();

  try {
    const successful = document.execCommand('copy');
    const msg = successful ? 'successful' : 'unsuccessful';
    console.log('Copying text command was ' + msg);
  } catch (err) {
    console.error('Oops, unable to copy', err);
  }

  document.body.removeChild(textArea);
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
                  fallbackCopyTextToClipboard(source);
                }
              },
            },
          ],
        }}
        trigger={['contextMenu']}
      >
        <SyntaxHighlighterWrapper>
          <pre>
            <code dangerouslySetInnerHTML={{ __html: highlightedHtml }} />
          </pre>
        </SyntaxHighlighterWrapper>
      </DropdownModal>
    </>
  );
}

export default ModDetailsSource;
