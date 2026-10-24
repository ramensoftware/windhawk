import i18n from 'i18next';
import Backend from 'i18next-http-backend';
import { initReactI18next } from 'react-i18next';

let initialized = false;
const defaultLanguage = 'en';

function i18nInitialize(language: string) {
  i18n
    // load translation using http -> see /public/locales (i.e. https://github.com/i18next/react-i18next/tree/master/example/react/public/locales)
    // learn more: https://github.com/i18next/i18next-http-backend
    .use(Backend)
    // pass the i18n instance to react-i18next.
    .use(initReactI18next)
    // init i18next
    // for all options read: https://www.i18next.com/overview/configuration-options
    .init({
      lng: language,
      fallbackLng: defaultLanguage,
      //debug: true,

      returnNull: false,
      returnEmptyString: false,

      interpolation: {
        escapeValue: false, // not needed for react as it escapes by default
      },

      react: {
        useSuspense: false,
      },

      backend: {
        // Use a relative load path.
        loadPath: './locales/{{lng}}/{{ns}}.json',
      },
    });
}

// The language as a BCP 47 tag, which Intl throws on otherwise. An underscore
// reads as a hyphen, as i18next reads it when it picks the strings, and a tag
// beyond that is the default language.
function wellFormedLanguage(language: string) {
  try {
    return Intl.getCanonicalLocales(language.replace(/_/g, '-'))[0];
  } catch {
    return defaultLanguage;
  }
}

export function setLanguage(language?: string) {
  const effectiveLanguage = wellFormedLanguage(language || defaultLanguage);

  if (initialized) {
    i18n.changeLanguage(effectiveLanguage);
  } else {
    i18nInitialize(effectiveLanguage);
    initialized = true;
  }

  // Set document direction
  document.documentElement.setAttribute('dir', i18n.dir(effectiveLanguage));
}
