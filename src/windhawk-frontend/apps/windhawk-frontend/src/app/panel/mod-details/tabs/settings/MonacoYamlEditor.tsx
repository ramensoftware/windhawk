import Editor, { loader } from '@monaco-editor/react';
import { ConfigProvider } from 'antd';
import * as monaco from 'monaco-editor/editor/editor.api.js';
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import styled from 'styled-components';
import { applyMonacoAppTheme, MONACO_APP_THEME } from '@app/monacoAppTheme';
import { useTheme } from '@app/theme';

// Configure Monaco Editor to use local npm package instead of CDN.
loader.config({ monaco });

const YamlEditorWrapper = styled.div`
  direction: ltr;
  margin-top: 12px;
`;

export interface MonacoYamlEditorProps {
  yamlText: string;
  onYamlTextChange: (value: string) => void;
  // Signals a re-measure: toggling fullscreen shifts the editor's top offset,
  // which its height is derived from (see measureCalcHeight).
  fullscreen?: boolean;
  // Whether a line too long for the editor is wrapped rather than scrolled to.
  wordWrap?: boolean;
}

function MonacoYamlEditor({
  yamlText,
  onYamlTextChange,
  fullscreen = false,
  wordWrap = false,
}: MonacoYamlEditorProps) {
  const { resolvedTheme } = useTheme();
  const [editorCalcHeight, setEditorCalcHeight] = useState('0');
  const editorRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);

  // The editor grows from its position down to the bottom of the viewport, in
  // both the inline and fullscreen layouts. Derive that height from its current
  // top offset (which already includes the 12px wrapper margin).
  const measureCalcHeight = useCallback(() => {
    const rect = editorRef.current?.getDomNode()?.getBoundingClientRect();
    if (!rect) {
      return;
    }
    const bottomOffset = 24; // Bottom padding
    const totalOffset = rect.top + bottomOffset;
    // Fallback for older browsers: use dvh if supported, otherwise vh
    const viewportUnit = CSS.supports('height: 100dvh') ? 'dvh' : 'vh';
    setEditorCalcHeight(`calc(100${viewportUnit} - ${totalOffset}px)`);
  }, []);

  // Monaco doesn't reflow on its own (automaticLayout is off to avoid a
  // ResizeObserver that spams "loop completed with undelivered notifications").
  // Re-fit it whenever the applied height changes, before paint to avoid a flash.
  useLayoutEffect(() => {
    editorRef.current?.layout();
  }, [editorCalcHeight]);

  // The editor's top offset (and thus its height) changes when the layout
  // context changes - toggling fullscreen moves it up/down. Re-measure and re-fit
  // before paint so the editor never shows a stale height for a frame.
  useLayoutEffect(() => {
    measureCalcHeight();
    editorRef.current?.layout();
  }, [fullscreen, measureCalcHeight]);

  // Keep the editor sized as the window (or the hosting VSCode panel) resizes.
  useEffect(() => {
    const handleResize = () => {
      measureCalcHeight();
      editorRef.current?.layout();
    };
    window.addEventListener('resize', handleResize);
    return () => window.removeEventListener('resize', handleResize);
  }, [measureCalcHeight]);

  // Match the editor background to the app background as the theme changes.
  useEffect(() => {
    applyMonacoAppTheme(resolvedTheme);
  }, [resolvedTheme]);

  return (
    <ConfigProvider direction="ltr">
      <YamlEditorWrapper>
        <Editor
          height={editorCalcHeight}
          defaultLanguage="yaml"
          value={yamlText}
          beforeMount={() => applyMonacoAppTheme(resolvedTheme)}
          onChange={(value) => {
            onYamlTextChange(value || '');
          }}
          onMount={(editor, monacoInstance) => {
            editorRef.current = editor;

            measureCalcHeight();

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
            wordWrap: wordWrap ? 'on' : 'off',
          }}
          theme={MONACO_APP_THEME}
        />
      </YamlEditorWrapper>
    </ConfigProvider>
  );
}

export default MonacoYamlEditor;
