import Editor, { loader } from '@monaco-editor/react';
import { ConfigProvider } from 'antd';
import * as monaco from 'monaco-editor/esm/vs/editor/editor.api.js';
import { useState } from 'react';
import styled from 'styled-components';

// Configure Monaco Editor to use local npm package instead of CDN.
loader.config({ monaco });

const YamlEditorWrapper = styled.div`
  direction: ltr;
  margin-top: 12px;
`;

export interface MonacoYamlEditorProps {
  yamlText: string;
  onYamlTextChange: (value: string) => void;
}

function MonacoYamlEditor({ yamlText, onYamlTextChange }: MonacoYamlEditorProps) {
  const [editorCalcHeight, setEditorCalcHeight] = useState('0');

  return (
    <ConfigProvider direction="ltr">
      <YamlEditorWrapper>
        <Editor
          height={editorCalcHeight}
          defaultLanguage="yaml"
          value={yamlText}
          onChange={(value) => {
            onYamlTextChange(value || '');
          }}
          onMount={(editor, monacoInstance) => {
            // Calculate height based on position
            const rect = editor.getDomNode()?.getBoundingClientRect();
            if (!rect) {
              return;
            }
            const topOffset = rect.top;
            const bottomOffset = 24; // Bottom padding
            const totalOffset = topOffset + bottomOffset;
            // Fallback for older browsers: use dvh if supported, otherwise vh
            const viewportUnit = CSS.supports('height: 100dvh') ? 'dvh' : 'vh';
            setEditorCalcHeight(`calc(100${viewportUnit} - ${totalOffset}px)`);

            // Monaco's context-menu Paste runs document.execCommand('paste'), which
            // Chromium-based webviews block (microsoft/monaco-editor#5068): Monaco
            // draws its own context menu, so the item never reaches the webview's
            // native edit menu the way plain inputs do. Replace it with an action that
            // reads through the async clipboard API.
            //
            // The scope differs by host. The Tauri shell (WebView2) handles Copy,
            // Cut, and keyboard paste (Ctrl+V, Shift+Insert) natively - only the
            // context-menu Paste is broken - so it gets just this menu action with no
            // keybinding, leaving the native shortcuts intact. The VSCode webview
            // (Electron) blocks all of them, so it also overrides Copy/Cut and binds
            // the paste keys below. The website bundles no Monaco, so EXTENSION (every
            // non-website build) is the right outer scope.

            /// #if EXTENSION
            /// #if !TAURI
            // Add copy action (Ctrl+C)
            editor.addAction({
              id: 'editor.action.clipboardCopyActionWithExecCommand',
              label: 'Copy',
              keybindings: [monacoInstance.KeyMod.CtrlCmd | monacoInstance.KeyCode.KeyC],
              precondition: 'editorTextFocus',
              contextMenuGroupId: '9_cutcopypaste',
              contextMenuOrder: 1,
              run: (ed) => {
                const selection = ed.getSelection();
                const model = ed.getModel();
                if (!selection || !model) return;

                if (selection.isEmpty()) {
                  // No selection - copy the entire current line including newline
                  const lineNumber = selection.startLineNumber;

                  // Select the line including the newline character
                  const lineRange = new monacoInstance.Range(
                    lineNumber, 1,
                    lineNumber + 1, 1
                  );
                  ed.setSelection(lineRange);
                  document.execCommand('copy');
                  // Restore cursor position
                  ed.setSelection(selection);
                } else {
                  // Has selection - copy selected text
                  document.execCommand('copy');
                }
              }
            });

            // Add cut action (Ctrl+X)
            editor.addAction({
              id: 'editor.action.clipboardCutActionWithExecCommand',
              label: 'Cut',
              keybindings: [monacoInstance.KeyMod.CtrlCmd | monacoInstance.KeyCode.KeyX],
              precondition: 'editorTextFocus',
              contextMenuGroupId: '9_cutcopypaste',
              contextMenuOrder: 0,
              run: (ed) => {
                const selection = ed.getSelection();
                const model = ed.getModel();
                if (!selection || !model) return;

                if (selection.isEmpty()) {
                  // No selection - cut the entire current line including newline
                  const lineNumber = selection.startLineNumber;

                  // Select the entire line including newline
                  const lineRange = new monacoInstance.Range(
                    lineNumber, 1,
                    lineNumber + 1, 1
                  );
                  ed.setSelection(lineRange);
                  document.execCommand('copy');

                  // Delete the entire line including newline
                  ed.executeEdits('cut', [{
                    range: lineRange,
                    text: '',
                    forceMoveMarkers: true
                  }]);
                } else {
                  // Has selection - cut selected text
                  document.execCommand('copy');
                  ed.executeEdits('cut', [{
                    range: selection,
                    text: '',
                    forceMoveMarkers: true
                  }]);
                }
              }
            });
            /// #endif

            // Add paste action (Ctrl+V)
            editor.addAction({
              id: 'editor.action.clipboardPasteActionWithExecCommand',
              label: 'Paste',
              /// #if !TAURI
              keybindings: [monacoInstance.KeyMod.CtrlCmd | monacoInstance.KeyCode.KeyV],
              /// #endif
              precondition: 'editorTextFocus',
              contextMenuGroupId: '9_cutcopypaste',
              contextMenuOrder: 2,
              run: async (ed) => {
                try {
                  // Try modern clipboard API first
                  if (navigator.clipboard && navigator.clipboard.readText) {
                    const text = await navigator.clipboard.readText();
                    if (text) {
                      const selection = ed.getSelection();
                      if (selection) {
                        ed.executeEdits('paste', [{
                          range: selection,
                          text: text,
                          forceMoveMarkers: true
                        }]);
                      }
                    }
                  } else {
                    // Fallback to execCommand
                    document.execCommand('paste');
                  }
                } catch (err) {
                  console.error('Paste failed:', err);
                }
              }
            });

            /// #if !TAURI
            // Add paste action for Shift+Insert
            editor.addAction({
              id: 'editor.action.clipboardPasteActionWithShiftInsert',
              label: 'Paste',
              keybindings: [monacoInstance.KeyMod.Shift | monacoInstance.KeyCode.Insert],
              precondition: 'editorTextFocus',
              run: async (ed) => {
                try {
                  if (navigator.clipboard && navigator.clipboard.readText) {
                    const text = await navigator.clipboard.readText();
                    if (text) {
                      const selection = ed.getSelection();
                      if (selection) {
                        ed.executeEdits('paste', [{
                          range: selection,
                          text: text,
                          forceMoveMarkers: true
                        }]);
                      }
                    }
                  } else {
                    document.execCommand('paste');
                  }
                } catch (err) {
                  console.error('Paste failed:', err);
                }
              }
            });
            /// #endif

            // Hide the default clipboard actions replaced above so they do not show
            // beside the custom ones in the context menu.
            // https://github.com/microsoft/monaco-editor/issues/1280#issuecomment-2099873176
            // Tauri only replaces Paste, so only Paste is removed there; the VSCode
            // webview also replaces Copy/Cut.
            const removableIds = [
              /// #if !TAURI
              'editor.action.clipboardCopyAction',
              'editor.action.clipboardCutAction',
              /// #endif
              'editor.action.clipboardPasteAction'
            ];
            // eslint-disable-next-line @typescript-eslint/no-explicit-any
            const contextmenu = editor.getContribution('editor.contrib.contextmenu') as any;
            if (contextmenu && contextmenu._getMenuActions) {
              const realMethod = contextmenu._getMenuActions;
              contextmenu._getMenuActions = function () {
                // eslint-disable-next-line prefer-rest-params
                const items = realMethod.apply(contextmenu, arguments);
                // eslint-disable-next-line @typescript-eslint/no-explicit-any
                return items.filter(function (item: any) {
                  return !removableIds.includes(item.id);
                });
              };
            }
            /// #endif
          }}
          options={{
            detectIndentation: false,
            tabSize: 2,
            insertSpaces: true,
            minimap: { enabled: false },
          }}
          theme="vs-dark"
        />
      </YamlEditorWrapper>
    </ConfigProvider>
  );
}

export default MonacoYamlEditor;
