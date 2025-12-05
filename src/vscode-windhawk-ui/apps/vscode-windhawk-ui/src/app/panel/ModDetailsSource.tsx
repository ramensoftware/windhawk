import { ConfigProvider, Switch } from 'antd';
import 'prism-themes/themes/prism-vsc-dark-plus.css';
import Prism from 'prismjs';
import 'prismjs/components/prism-c';
import 'prismjs/components/prism-cpp';
import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { DropdownModal, dropdownModalDismissed } from '../components/InputWithContextMenu';

const SyntaxHighlighterWrapper = styled.div`
  direction: ltr;

  pre {
    font-size: 13px;
    line-height: 1.5;
    background-color: #1e1e1e;
    padding: 12px;
    border-radius: 2px;
    overflow: auto;
  }

  code {
    color: #d4d4d4;
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

  const highlightedHtml = useMemo(() => {
    return Prism.highlight(currentSource, Prism.languages['cpp'], 'cpp');
  }, [currentSource]);

  return (
    <ConfigProvider direction="ltr">
      <ConfigurationWrapper>
        <span>{t('modDetails.code.collapseExtra')}</span>
        <Switch
          checked={isCollapsed}
          onChange={(checked) => setIsCollapsed(checked)}
        />
      </ConfigurationWrapper>
      <DropdownModal
        menu={{
          items: [
            {
              label: t('general.copy'),
              key: 'copy',
              onClick: () => {
                dropdownModalDismissed();
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
    </ConfigProvider>
  );
}

export default ModDetailsSource;
