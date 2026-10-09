import { isWireError, showErrorMessage, showInfoMessage } from '@app/feedback';
import { useModSettings } from '@app/panel/mod-details/useModSettings';
import {
  useCancelCaptureHotkey,
  useCaptureHotkey,
  useGetModDynamicSelectOptions,
  useHotkeyCaptureProgress,
  useListFontFamilies,
  usePickFilePath,
} from '@app/webviewIPC';
import { readStoredValue, writeStoredValue } from '@app/utils';
import {
  type DynamicSelectOption,
  type HotkeyCaptureProgressEventData,
  type InitialSettings,
} from '@app/webviewIPCMessages';
import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from 'react';
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
import { schemaHasDynamicSelect } from './core/dropdownOptions';
import { type HotkeyModifiers } from './core/hotkey';
import { schemaHasFormat } from './core/schemaQuery';
import { flattenAllDefaults, isSettingModified } from './core/settingDefaults';
import { canonicalSettings } from './core/settingValues';
import { readSavedYaml, saveYaml } from './core/yamlStorage';

const MODE_STORAGE_KEY = 'settingsYamlMode';

// One empty map for every mod nothing has been read for, so a form drawn from
// it is not redrawn for a fresh empty one each render.
const NO_DYNAMIC_SELECT_OPTIONS: Record<string, DynamicSelectOption[]> = {};

// Likewise for the font families before the host has answered.
const NO_FONT_FAMILIES: string[] = [];

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
 * How a hotkey capture the badge asked for ended: with a shortcut in the stored
 * form; with none, the capture having been canceled, having lost focus or timed
 * out, or the request abandoned; or refused - the host cannot record on this
 * machine, and the editor has switched to its manual editor.
 */
export type HotkeyCaptureOutcome =
  | { kind: 'hotkey'; hotkey: string }
  | { kind: 'canceled' }
  | { kind: 'refused' };

/**
 * The hotkey capture a `hotkey` setting's badge records through. `mode` is the
 * editor's, shared by every hotkey setting it draws: `primary` records through
 * the host, `fallback` opens a manual editor instead, which the editor switches
 * to for its lifetime the first time the host refuses to start a capture.
 * `held` is what the capture in flight reports held, null while none is.
 */
export type HotkeyCaptureModel = {
  mode: 'primary' | 'fallback';
  held: HotkeyModifiers | null;
  start: () => Promise<HotkeyCaptureOutcome>;
  cancel: () => void;
};

/**
 * The presentational slice the settings View renders. Both the app hook
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
  // What the mod wrote at runtime for its `$dynamicSelect` settings, keyed by
  // setting path, and the way to ask for it again - which a dynamic dropdown
  // does as it opens, so a device plugged in since the form came up is offered.
  dynamicSelectOptions: Record<string, DynamicSelectOption[]>;
  onRefreshDynamicSelectOptions: () => void;
  // Runs the host's Open dialog for a `filePath` setting - its folder dialog
  // for a `folderPath` one - and resolves with the path picked, or null when
  // nothing was: the dialog was dismissed, it failed (reported through the
  // usual failure surface), or the editor is gone.
  onPickFilePath: (
    settingKey: string,
    currentPath: string,
    folder?: boolean
  ) => Promise<string | null>;
  // The font families installed on the host, for a `fontFamily` setting's
  // completion: read once when the editor comes up for a schema declaring one,
  // and empty until then, or when the host could not list them.
  fontFamilies: string[];
  // The capture a `hotkey` setting's badge records a shortcut through.
  hotkeyCapture: HotkeyCaptureModel;
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
 * Owns the whole mod settings editing lifecycle for the app: takes the
 * settings as useModSettings reads them, on mount and again whenever they may
 * have moved on disk, keeps the single source of truth (form draft or YAML
 * buffer), derives dirtiness, and drives an explicit save round-trip that
 * persists the hand-formatted YAML only once the backend confirms.
 *
 * App-only - it uses the settings IPC hooks, which are unavailable in
 * website builds. `readOnly` covers the app's non-installed (preview)
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
  // closed over: the mod the editor is on.
  const modIdRef = useRef(modId);
  useEffect(() => {
    modIdRef.current = modId;
  });

  // A read of the settings. The first installs the editor, honoring the
  // persisted YAML/form mode preference. A later one is fitted in by the
  // reducer, against the editor as it stands when the read lands.
  const onRead = useCallback(
    (settings: ModSettings, first: boolean) => {
      if (first) {
        const startInYaml = readStoredValue(MODE_STORAGE_KEY) === 'true';
        const yamlText = startInYaml ? resolveInitialYaml(modId, settings, yamlDeps) : null;
        const working =
          yamlText !== null ? makeYamlWorking(yamlText, settings) : makeUiWorking(settings);

        dispatch({ type: 'loaded', saved: settings, working });
        return;
      }

      dispatch({ type: 'refreshed', settings, canonical, toYaml: settingsToYaml });
    },
    [modId, yamlDeps, canonical, settingsToYaml]
  );

  const { save: saveSettings, saving } = useModSettings(modId, {
    enabled: !readOnly,
    onRead,
  });
  const { getModDynamicSelectOptions } = useGetModDynamicSelectOptions();
  const { pickFilePath } = usePickFilePath();
  const { listFontFamilies } = useListFontFamilies();
  const { captureHotkey } = useCaptureHotkey();
  const { cancelCaptureHotkey } = useCancelCaptureHotkey();

  // The last options read, kept with the mod they were read for: the mod on
  // screen offers them only while it is that mod, so nothing of the mod before
  // is offered while a read for this one is out.
  const [dynamicSelectOptionsRead, setDynamicSelectOptionsRead] = useState<{
    modId: string;
    options: Record<string, DynamicSelectOption[]>;
  } | null>(null);
  const dynamicSelectOptions =
    dynamicSelectOptionsRead?.modId === modId
      ? dynamicSelectOptionsRead.options
      : NO_DYNAMIC_SELECT_OPTIONS;

  // The whole map is replaced on every reply: a device that has gone is gone
  // from the list. A read the host could not make replaces nothing - the last
  // list read is closer to what the mod wrote than none at all, and the failure
  // is reported on its own.
  const refreshDynamicSelectOptions = useCallback(async () => {
    const result = await getModDynamicSelectOptions({ modId });
    if (
      result.status !== 'reply' ||
      result.data.modId !== modIdRef.current ||
      isWireError(result.data.error)
    ) {
      return;
    }
    setDynamicSelectOptionsRead({ modId: result.data.modId, options: result.data.options });
  }, [getModDynamicSelectOptions, modId]);

  const onRefreshDynamicSelectOptions = useCallback(() => {
    void refreshDynamicSelectOptions();
  }, [refreshDynamicSelectOptions]);

  // Fetched beside the settings, for a mod with something to fetch: a schema
  // with no dynamic dropdown has nothing to draw the options in, and the
  // preview draws the declared options only.
  const hasDynamicSelect = useMemo(
    () => schemaHasDynamicSelect(initialSettings),
    [initialSettings]
  );

  useEffect(() => {
    if (readOnly || !hasDynamicSelect) {
      return;
    }
    void refreshDynamicSelectOptions();
  }, [readOnly, hasDynamicSelect, refreshDynamicSelectOptions]);

  // Read once per editor for a schema with a font setting to complete: the
  // installed families change too rarely to ask again, and the preview draws
  // the declared name alone.
  const hasFontFamily = useMemo(
    () => schemaHasFormat(initialSettings, 'fontFamily'),
    [initialSettings]
  );
  const [fontFamilies, setFontFamilies] = useState<string[]>(NO_FONT_FAMILIES);

  useEffect(() => {
    if (readOnly || !hasFontFamily) {
      return;
    }
    void (async () => {
      const result = await listFontFamilies({});
      // A list the host could not make leaves the control to free text, which
      // it takes either way; the failure is reported on its own.
      if (result.status !== 'reply' || isWireError(result.data.error)) {
        return;
      }
      setFontFamilies(result.data.families);
    })();
  }, [readOnly, hasFontFamily, listFontFamilies]);

  // The hotkey capture. Primary until the host refuses to start one - a core
  // that does not know the command, a keyboard hook it could not install -
  // which switches every hotkey badge of this editor to the manual editor, and
  // says so once. There is no probe at mount: the first click is the probe,
  // and a refusal lands the user in the editor they need.
  const [hotkeyCaptureMode, setHotkeyCaptureMode] = useState<'primary' | 'fallback'>('primary');
  const [hotkeyHeld, setHotkeyHeld] = useState<HotkeyModifiers | null>(null);
  // Whether a capture is out, so a progress event that arrives outside one (a
  // capture another editor started, a late one) draws nothing here; and
  // whether the host has refused one, read at the refusal rather than from
  // the state so the notice goes out once.
  const hotkeyCapturingRef = useRef(false);
  const hotkeyRefusedRef = useRef(false);

  useHotkeyCaptureProgress(
    useCallback((data: HotkeyCaptureProgressEventData) => {
      if (hotkeyCapturingRef.current) {
        setHotkeyHeld(data.modifiers);
      }
    }, [])
  );

  const startHotkeyCapture = useCallback(async (): Promise<HotkeyCaptureOutcome> => {
    hotkeyCapturingRef.current = true;
    setHotkeyHeld(null);
    const result = await captureHotkey({});
    hotkeyCapturingRef.current = false;
    setHotkeyHeld(null);
    if (result.status !== 'reply') {
      return { kind: 'canceled' };
    }
    if (isWireError(result.data.error)) {
      if (!hotkeyRefusedRef.current) {
        hotkeyRefusedRef.current = true;
        setHotkeyCaptureMode('fallback');
        showInfoMessage(t('modDetails.settings.hotkeyCaptureUnavailable'));
      }
      return { kind: 'refused' };
    }
    if (result.data.hotkey === null) {
      return { kind: 'canceled' };
    }
    return { kind: 'hotkey', hotkey: result.data.hotkey };
  }, [captureHotkey, t]);

  const cancelHotkeyCapture = useCallback(() => {
    if (hotkeyCapturingRef.current) {
      void cancelCaptureHotkey({});
    }
  }, [cancelCaptureHotkey]);

  // An editor that goes away mid-capture takes the capture with it: the hook
  // takes every key until it ends, and nothing would be there to take its
  // answer.
  useEffect(() => cancelHotkeyCapture, [cancelHotkeyCapture]);

  const hotkeyCapture = useMemo<HotkeyCaptureModel>(
    () => ({
      mode: hotkeyCaptureMode,
      held: hotkeyHeld,
      start: startHotkeyCapture,
      cancel: cancelHotkeyCapture,
    }),
    [hotkeyCaptureMode, hotkeyHeld, startHotkeyCapture, cancelHotkeyCapture]
  );

  const onPickFilePath = useCallback(
    async (settingKey: string, currentPath: string, folder?: boolean): Promise<string | null> => {
      const result = await pickFilePath({
        modId,
        settingKey,
        ...(currentPath ? { currentPath } : {}),
        ...(folder ? { folder: true } : {}),
      });
      // The reply names no mod, so the request's own is held against the mod
      // the editor is on, as the other replies are.
      if (
        result.status !== 'reply' ||
        modIdRef.current !== modId ||
        result.data.path === undefined
      ) {
        return null;
      }
      return result.data.path;
    },
    [pickFilePath, modId]
  );

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

    if (!(await saveSettings(settingsToSave))) {
      return false;
    }

    if (savedText !== undefined) {
      saveYaml(modId, savedText);
    }
    dispatch({ type: 'saveSucceeded', savedSettings: settingsToSave, savedText });

    return true;
  }, [state, canonical, yamlToSettings, modId, saveSettings]);

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
      initialSettings.some((item) =>
        isSettingModified(draft, item.value, item.key, item.float)
      ));

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
    dynamicSelectOptions,
    onRefreshDynamicSelectOptions,
    onPickFilePath,
    fontFamilies,
    hotkeyCapture,
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
    isSaving: saving,
    viewProps,
    save,
  };
}
