import useSWR from 'swr';
import * as yaml from 'js-yaml';
import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { fetchText } from '@app/utils/swrHelpers';
import type {
  InitialSettings,
  InitialSettingItem,
  ModMetadata,
  RepositoryDetails,
} from '@app/webviewIPCMessages';
import { ModDetailsView, type ModSourceData } from './ModDetails.View';
import { findCommentBlock } from './modSourceBlocks';

type RepositoryModDetails = {
  metadata?: ModMetadata;
  details?: RepositoryDetails;
};

interface Props {
  modId: string;
  repositoryModDetails?: RepositoryModDetails;
  goBack?: () => void;
}

/**
 * Get the best language match from a list of localized values.
 * Priority: exact match > more specific match > iteratively less specific > null language (default)
 */
function getBestLanguageMatch<T>(
  matchLanguage: string,
  candidates: Array<{ language: string | null; value: T }>
): { language: string | null; value: T } {
  const languages = candidates.map((x) => x.language?.toLowerCase() ?? null);

  // Build list of language variants to try: "en-us" -> ["en-us", "en"]
  const languagesToTry: string[] = [];
  let iterLanguage = matchLanguage.toLowerCase();
  languagesToTry.push(iterLanguage);
  while (iterLanguage.includes('-')) {
    iterLanguage = iterLanguage.replace(/-[^-]*$/, '');
    languagesToTry.push(iterLanguage);
  }

  for (const langToTry of languagesToTry) {
    // Exact match (case-insensitive)
    const exactIndex = languages.indexOf(langToTry);
    if (exactIndex !== -1) {
      return candidates[exactIndex];
    }

    // A more specific language (e.g., "en" matches "en-us")
    const prefix = langToTry + '-';
    const moreSpecificIndex = languages.findIndex(
      (lang) => lang !== null && lang.startsWith(prefix)
    );
    if (moreSpecificIndex !== -1) {
      return candidates[moreSpecificIndex];
    }
  }

  // Fall back to default (null language)
  const defaultIndex = languages.indexOf(null);
  if (defaultIndex !== -1) {
    return candidates[defaultIndex];
  }

  // Last resort: return first item
  return candidates[0];
}

/**
 * Extract and parse initial settings from mod source.
 * Adapted from the extension backend's InitialSettings parser.
 */
function extractInitialSettings(
  modSource: string,
  language: string
): InitialSettings | null {
  const settingsBlock = findCommentBlock(modSource, 'WindhawkModSettings');
  if (settingsBlock === null) {
    return null;
  }

  try {
    const settings = yaml.load(settingsBlock);

    if (!Array.isArray(settings)) {
      return null;
    }

    const parseSettings = (
      settingsArray: Record<string, unknown>[]
    ): InitialSettings => {
      return settingsArray.map(parseSettingItem);
    };

    const parseSettingItem = (
      value: Record<string, unknown>
    ): InitialSettingItem => {
      // Find the actual setting key (not starting with $)
      const actualParameters = Object.keys(value).filter(
        (x) => !x.startsWith('$')
      );
      if (actualParameters.length === 0) {
        throw new Error('Missing settings key');
      } else if (actualParameters.length > 1) {
        throw new Error('More than one settings key');
      }

      const actualParameter = actualParameters[0];
      const metaParameters = Object.keys(value).filter((x) =>
        x.startsWith('$')
      );

      // Group meta parameters by their base name (name, description, options)
      const metaGroups: Record<
        string,
        Array<{ language: string | null; value: unknown }>
      > = {};

      for (const paramWithPrefix of metaParameters) {
        const param = paramWithPrefix.slice(1); // remove '$'
        const paramParts = param.split(':');
        const baseName = paramParts[0];
        const lang = paramParts[1] ?? null;

        metaGroups[baseName] = metaGroups[baseName] ?? [];
        metaGroups[baseName].push({
          language: lang,
          value: value[paramWithPrefix],
        });
      }

      // Select best language match for each meta parameter
      const result: Record<string, unknown> = {};
      for (const key of Object.keys(metaGroups)) {
        result[key] = getBestLanguageMatch(language, metaGroups[key]).value;
      }

      result['key'] = actualParameter;
      result['value'] = parseSettingValue(value[actualParameter]);

      return result as InitialSettingItem;
    };

    const parseSettingValue = (value: unknown): InitialSettingItem['value'] => {
      if (
        typeof value === 'boolean' ||
        typeof value === 'number' ||
        typeof value === 'string'
      ) {
        return value;
      }

      if (Array.isArray(value)) {
        if (value.length === 0) {
          throw new Error('Empty array settings value');
        }

        const firstItem = value[0];
        if (typeof firstItem === 'number' || typeof firstItem === 'string') {
          return value as number[] | string[];
        }

        // Array of objects - could be nested settings or array of nested settings
        if (Array.isArray(firstItem)) {
          // Array of arrays of settings
          return value.map((item) =>
            parseSettings(item as Record<string, unknown>[])
          );
        }

        // Array of settings objects
        return parseSettings(value as Record<string, unknown>[]);
      }

      throw new Error('Invalid settings value type');
    };

    return parseSettings(settings as Record<string, unknown>[]);
  } catch (e) {
    console.error('Failed to parse settings:', e);
    return null;
  }
}

export function ModDetailsWebsite({ modId, repositoryModDetails, goBack }: Props) {
  const { i18n } = useTranslation();

  // Fetch mod source from web
  const {
    data: onlineModSource,
    error: onlineModSourceError,
    mutate: refetchModSource,
  } = useSWR(`https://mods.windhawk.net/mods/${modId}.wh.cpp`, fetchText);

  // Derive data from SWR, memoized to avoid re-parsing on every render
  const modSourceData: ModSourceData | null = useMemo(() => {
    if (!onlineModSource) {
      // A fetch that failed is a read that failed, and it says so the way the
      // host says it: every field null. Absent on its own is a read still on its
      // way, which the view waits on with a spinner nothing ever ends. A failure
      // alongside a source that did arrive is a revalidation that did not stick,
      // and the source stands.
      return onlineModSourceError
        ? { source: null, metadata: null, readme: null, initialSettings: null }
        : null;
    }
    return {
      source: onlineModSource,
      metadata: repositoryModDetails?.metadata ?? {},
      readme: findCommentBlock(onlineModSource, 'WindhawkModReadme'),
      initialSettings: extractInitialSettings(onlineModSource, i18n.language),
    };
  }, [
    onlineModSource,
    onlineModSourceError,
    repositoryModDetails?.metadata,
    i18n.language,
  ]);

  return (
    <ModDetailsView
      modId={modId}
      goBack={goBack}
      modMetadata={repositoryModDetails?.metadata ?? {}}
      repositoryDetails={repositoryModDetails?.details}
      modSourceData={modSourceData}
      selectedModSourceData={modSourceData}
      onRetryLoad={() => {
        refetchModSource();
      }}
    />
  );
}
