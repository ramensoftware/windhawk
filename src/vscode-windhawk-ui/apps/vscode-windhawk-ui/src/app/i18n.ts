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

export function setLanguage(language?: string) {
  if (initialized) {
    i18n.changeLanguage(language || defaultLanguage);
    return;
  }

  i18nInitialize(language || defaultLanguage);
  initialized = true;
}
