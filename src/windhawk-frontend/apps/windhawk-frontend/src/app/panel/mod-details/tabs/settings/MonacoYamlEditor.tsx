import Editor, { loader } from '@monaco-editor/react';
import { ConfigProvider } from 'antd';
import * as monaco from 'monaco-editor/editor/editor.api.js';
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import styled from 'styled-components';
import { applyMonacoAppTheme, MONACO_APP_THEME } from '@app/monacoAppTheme';
import { registerMonacoArgbColors } from '@app/monacoArgbColors';
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
  // Every text this editor has handed out and not yet seen come back. The prop is
  // fed from these, so an incoming value that appears here is this editor's own
  // echo, however many renders late, and must not be written back.
  const pendingEmits = useRef<string[]>([]);

  // A stable object: the editor re-applies whatever this holds on every render it
  // changes identity across, which is every render when it is written inline.
  const editorOptions = useMemo<monaco.editor.IStandaloneEditorConstructionOptions>(
    () => ({
      detectIndentation: false,
      tabSize: 2,
      insertSpaces: true,
      minimap: { enabled: false },
      wordWrap: wordWrap ? 'on' : 'off',
    }),
    [wordWrap]
  );

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

  // The editor owns its text; the prop only carries changes made elsewhere (a
  // revert, a mode switch, another mod). Recognising an echo by what was emitted,
  // rather than by comparing against the model as @monaco-editor/react's `value`
  // does, is what makes this exact: a render carrying an older text still matches
  // an entry here, so typing never provokes a write. That matters beyond the
  // stale badges - the write it avoids replaces the whole document, which
  // collapses every tracked decoration onto one point and discards whatever was
  // typed after the render it came from.
  useEffect(() => {
    const editor = editorRef.current;
    if (!editor) {
      return;
    }
    const echoed = pendingEmits.current.indexOf(yamlText);
    if (echoed !== -1) {
      pendingEmits.current.splice(0, echoed + 1);
      return;
    }
    if (yamlText === editor.getValue()) {
      return;
    }
    pendingEmits.current.length = 0;
    editor.setValue(yamlText);
  }, [yamlText]);

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
          defaultValue={yamlText}
          beforeMount={() => {
            applyMonacoAppTheme(resolvedTheme);
            registerMonacoArgbColors();
          }}
          onChange={(value) => {
            const text = value || '';
            pendingEmits.current.push(text);
            onYamlTextChange(text);
          }}
          onMount={(editor, monacoInstance) => {
            editorRef.current = editor;

            measureCalcHeight();

            // Monaco's built-in clipboard actions do not work in the VSCode webview
            // (Electron), which blocks the clipboard commands they run
            // (microsoft/monaco-editor#5068), and because Monaco draws its own context
            // menu, the entries never reach the webview's native edit menu the way plain
            // inputs do. Replace them, and bind the paste keys the webview swallows too.
            //
            // The Tauri shell (WebView2) drives the clipboard natively and the website
            // bundles no Monaco, so this is VSCode-only.

            /// #if EXTENSION && !TAURI
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

            // Add paste action (Ctrl+V)
            editor.addAction({
              id: 'editor.action.clipboardPasteActionWithExecCommand',
              label: 'Paste',
              keybindings: [monacoInstance.KeyMod.CtrlCmd | monacoInstance.KeyCode.KeyV],
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

            // Hide the default clipboard actions replaced above so they do not show
            // beside the custom ones in the context menu.
            // https://github.com/microsoft/monaco-editor/issues/1280#issuecomment-2099873176
            const removableIds = [
              'editor.action.clipboardCopyAction',
              'editor.action.clipboardCutAction',
              'editor.action.clipboardPasteAction',
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
          options={editorOptions}
          theme={MONACO_APP_THEME}
        />
      </YamlEditorWrapper>
    </ConfigProvider>
  );
}

export default MonacoYamlEditor;
