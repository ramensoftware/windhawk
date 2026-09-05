import { showErrorMessage } from '@app/feedback';
import { useGetModSettings, useSetModSettings } from '@app/webviewIPC';
import { readStoredValue, writeStoredValue } from '@app/utils';
import { type InitialSettings } from '@app/webviewIPCMessages';
import { useCallback, useEffect, useMemo, useReducer, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { type ModSettings, YamlConverter, YamlSchemaValidator } from './core/yamlConverter';
import {
  editorReducer,
  initialEditorState,
  isDirty,
  isYamlEdited,
  makeUiWorking,
  makeYamlWorking,
  resolveInitialYaml,
  type EditorState,
  type ResolveInitialYamlDeps,
} from './core/editorState';
import { flattenAllDefaults, isSettingModified } from './core/settingDefaults';
import { canonicalSettings } from './core/settingValues';
import { readSavedYaml, saveYaml } from './core/yamlStorage';

const MODE_STORAGE_KEY = 'settingsYamlMode';

const YamlErrorContent = styled.div`
  display: inline-block;
  text-align: start;
  font-family: 'Consolas', 'Monaco', 'Courier New', monospace;
  white-space: break-spaces;
`;

/**
 * Renders a (possibly multiline) YAML error message for the Ant Design message
 * component, keeping line breaks.
 */
function formatYamlError(error: string): React.ReactNode {
  const lines = error.split('\n');
  return (
    <YamlErrorContent>
      {lines.map((line, index) => (
        <span key={index}>
          {line}
          {index < lines.length - 1 && <br />}
        </span>
      ))}
    </YamlErrorContent>
  );
}

/**
 * The presentational slice the settings View renders. Both the extension hook
 * and the website's static read-only wrapper produce this shape, so the View
 * itself holds no editing state.
 */
export type EditorViewModel = {
  mode: 'ui' | 'yaml';
  draft: ModSettings;
  // The draft and the values the mod is saved with, both in the canonical form
  // an unsaved edit is judged in, which is what a row is marked against. Not
  // what the form renders from - that is draft, holding the values as they are.
  canonicalDraft: ModSettings;
  canonicalSaved: ModSettings;
  arrayMaxIndex: Record<string, number>;
  yamlText: string;
  isDirty: boolean;
  // Whether anything at all differs from the values the mod declares, which is
  // what there is to offer a whole-form revert for.
  anySettingModified: boolean;
  yamlAvailable: boolean;
  onChangeSetting: (key: string, value: string | number) => void;
  onAddArrayItem: (prefix: string, index: number) => void;
  onRemoveArrayItem: (prefix: string, index: number) => void;
  onRemoveAllArrayItems: (prefix: string) => void;
  // Moves the element at `from` to `to`, the rest of the array closing around it.
  onMoveArrayItem: (prefix: string, from: number, to: number) => void;
  // Puts the subtree at the given key back to the mod's declared defaults. The
  // empty key resets every setting.
  onResetSetting: (keyPrefix: string) => void;
  onSetYamlText: (text: string) => void;
  onToggleMode: () => void;
  onSave: () => void;
};

export type UseModSettingsEditor = {
  ready: boolean;
  isDirty: boolean;
  isSaving: boolean;
  viewProps: EditorViewModel;
  save: () => Promise<boolean>;
};

/**
 * Owns the whole mod settings editing lifecycle for the extension: fetches the
 * settings, keeps the single source of truth (form draft or YAML buffer),
 * derives dirtiness, and drives an explicit save round-trip that persists the
 * hand-formatted YAML only once the backend confirms.
 *
 * Extension-only - it uses the settings IPC hooks, which are unavailable in
 * website builds. `readOnly` covers the extension's non-installed (preview)
 * views: it starts ready with empty settings, never fetches, and disables YAML.
 */
export function useModSettingsEditor(
  modId: string,
  initialSettings: InitialSettings,
  options?: { readOnly?: boolean }
): UseModSettingsEditor {
  const { t } = useTranslation();
  const readOnly = options?.readOnly ?? false;

  const [state, dispatch] = useReducer(
    editorReducer,
    readOnly,
    (ro): EditorState =>
      ro ? { status: 'ready', saved: {}, working: makeUiWorking({}) } : initialEditorState
  );

  const yamlValidator = useMemo(
    () => new YamlSchemaValidator(initialSettings),
    [initialSettings]
  );

  const settingDefaults = useMemo(
    () => flattenAllDefaults(initialSettings),
    [initialSettings]
  );

  const canonical = useCallback(
    (settings: ModSettings) => canonicalSettings(settings, initialSettings),
    [initialSettings]
  );

  // Settings with no YAML rendering leave the editor with no buffer to show, and
  // null is how the uses below keep it in the form instead. An empty buffer would
  // read as a mod with no settings and be saved as the values it does have,
  // cleared.
  const settingsToYaml = useCallback(
    (settings: ModSettings): string | null => {
      try {
        return YamlConverter.toYaml(settings, initialSettings);
      } catch (error) {
        console.error('Error converting settings to YAML:', error);
        return null;
      }
    },
    [initialSettings]
  );

  const yamlToSettings = useCallback(
    (yamlString: string, sourceSettings: ModSettings) =>
      YamlConverter.fromYaml(yamlString, yamlValidator, t, sourceSettings),
    [yamlValidator, t]
  );

  const yamlDeps = useMemo<ResolveInitialYamlDeps>(
    () => ({ readSavedYaml, settingsToYaml, yamlToSettings }),
    [settingsToYaml, yamlToSettings]
  );

  // What a reply is judged against when it lands, rather than what its request
  // closed over: the mod the editor is on, and the converters as they stand.
  const modIdRef = useRef(modId);
  const yamlDepsRef = useRef(yamlDeps);
  useEffect(() => {
    modIdRef.current = modId;
    yamlDepsRef.current = yamlDeps;
  });

  const { getModSettings } = useGetModSettings();
  const { setModSettings, setModSettingsPending } = useSetModSettings();

  // Fetch settings on mount (edit mode only). The reply installs the initial
  // working state, honoring the persisted YAML/form mode preference.
  useEffect(() => {
    if (readOnly) {
      return;
    }

    void (async () => {
      const result = await getModSettings({ modId });
      // Nothing to install from a request the unmount abandoned, or from a read
      // of a mod the editor has since left.
      if (result.status !== 'reply' || result.data.modId !== modIdRef.current) {
        return;
      }

      const settings = result.data.settings;
      const startInYaml = readStoredValue(MODE_STORAGE_KEY) === 'true';
      const yamlText = startInYaml
        ? resolveInitialYaml(modId, settings, yamlDepsRef.current)
        : null;
      const working =
        yamlText !== null ? makeYamlWorking(yamlText, settings) : makeUiWorking(settings);

      dispatch({ type: 'loaded', saved: settings, working });
    })();
  }, [getModSettings, modId, readOnly]);

  const save = useCallback(async (): Promise<boolean> => {
    if (state.status !== 'ready' || !isDirty(state, canonical)) {
      return false;
    }

    const { working } = state;
    let settingsToSave: ModSettings;
    let savedText: string | undefined;

    if (working.mode === 'yaml') {
      if (isYamlEdited(working)) {
        const { settings, error } = yamlToSettings(working.text, working.sourceDraft);
        if (error || !settings) {
          showErrorMessage(formatYamlError(error ?? 'Unknown error'));
          return false;
        }
        settingsToSave = settings;
      } else {
        // The buffer was not hand-edited, so the seed draft is authoritative
        // (and lossless - it keeps values the YAML render would trim).
        settingsToSave = working.sourceDraft;
      }
      savedText = working.text;
    } else {
      settingsToSave = working.draft;
    }

    const result = await setModSettings({ modId, settings: settingsToSave });
    // The reply is this save's own, so all that is left to ask is whether it
    // still applies: an abandoned request reports nothing, and a mod the editor
    // has left is not one to take a saved baseline from.
    if (result.status !== 'reply' || result.data.modId !== modIdRef.current) {
      return false;
    }

    if (!result.data.succeeded) {
      return false;
    }

    if (savedText !== undefined) {
      saveYaml(modId, savedText);
    }
    dispatch({ type: 'saveSucceeded', savedSettings: settingsToSave, savedText });

    return true;
  }, [state, canonical, yamlToSettings, modId, setModSettings]);

  const toggleMode = useCallback(() => {
    if (state.status !== 'ready') {
      return;
    }

    const { working } = state;
    if (working.mode === 'ui') {
      const text = resolveInitialYaml(modId, working.draft, yamlDeps);
      if (text === null) {
        return;
      }
      dispatch({ type: 'enterYamlMode', text });
      writeStoredValue(MODE_STORAGE_KEY, 'true');
      return;
    }

    if (isYamlEdited(working)) {
      const { settings, error } = yamlToSettings(working.text, working.sourceDraft);
      if (error || !settings) {
        showErrorMessage(formatYamlError(error ?? 'Unknown error'));
        return;
      }
      dispatch({ type: 'exitYamlMode', draft: settings });
    } else {
      dispatch({ type: 'exitYamlMode', draft: working.sourceDraft });
    }
    writeStoredValue(MODE_STORAGE_KEY, 'false');
  }, [state, modId, yamlDeps, yamlToSettings]);

  const onChangeSetting = useCallback(
    (key: string, value: string | number) => dispatch({ type: 'changeSetting', key, value }),
    []
  );
  const onAddArrayItem = useCallback(
    (prefix: string, index: number) => dispatch({ type: 'addArrayItem', prefix, index }),
    []
  );
  const onRemoveArrayItem = useCallback(
    (prefix: string, index: number) => dispatch({ type: 'removeArrayItem', prefix, index }),
    []
  );
  const onRemoveAllArrayItems = useCallback(
    (prefix: string) => dispatch({ type: 'removeAllArrayItems', prefix }),
    []
  );
  const onMoveArrayItem = useCallback(
    (prefix: string, from: number, to: number) =>
      dispatch({ type: 'moveArrayItem', prefix, from, to }),
    []
  );
  // YAML mode edits text rather than rows, so a revert lands there as a buffer
  // written from the defaults - and only the whole-form one is reachable, there
  // being no rows to revert one at a time.
  const onResetSetting = useCallback(
    (keyPrefix: string) => {
      if (state.status === 'ready' && state.working.mode === 'yaml') {
        if (keyPrefix === '') {
          const text = settingsToYaml(settingDefaults);
          if (text !== null) {
            dispatch({ type: 'setYamlText', text });
          }
        }
        return;
      }
      dispatch({ type: 'resetSetting', keyPrefix, defaults: settingDefaults });
    },
    [state, settingDefaults, settingsToYaml]
  );
  const onSetYamlText = useCallback(
    (text: string) => dispatch({ type: 'setYamlText', text }),
    []
  );
  const onSave = useCallback(() => {
    void save();
  }, [save]);

  const dirty = isDirty(state, canonical);

  const working = state.status === 'ready' ? state.working : null;
  const draft = working?.mode === 'ui' ? working.draft : {};

  // What the whole-form revert is offered against. Telling what a YAML buffer
  // holds means parsing it, which is not worth doing on every render to decide
  // whether to show a button - so in that mode the revert is always offered.
  const anySettingModified =
    !readOnly &&
    (working?.mode !== 'ui' ||
      initialSettings.some((item) => isSettingModified(draft, item.value, item.key)));

  const viewProps: EditorViewModel = {
    mode: working?.mode ?? 'ui',
    draft,
    canonicalDraft: canonical(draft),
    canonicalSaved: canonical(state.status === 'ready' ? state.saved : {}),
    arrayMaxIndex: working?.mode === 'ui' ? working.arrayMaxIndex : {},
    yamlText: working?.mode === 'yaml' ? working.text : '',
    isDirty: dirty,
    anySettingModified,
    yamlAvailable: !readOnly,
    onChangeSetting,
    onAddArrayItem,
    onRemoveArrayItem,
    onRemoveAllArrayItems,
    onMoveArrayItem,
    onResetSetting,
    onSetYamlText,
    onToggleMode: toggleMode,
    onSave,
  };

  return {
    ready: state.status === 'ready',
    isDirty: dirty,
    isSaving: setModSettingsPending,
    viewProps,
    save,
  };
}
