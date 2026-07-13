import pluginCypress from 'eslint-plugin-cypress';
import baseConfig from '../../eslint.config.mjs';

export default [
  ...baseConfig,
  pluginCypress.configs.recommended,
  {
    files: ['**/*.ts', '**/*.tsx', '**/*.js', '**/*.jsx'],
    // Override or add rules here
    rules: {},
  },
  {
    files: ['src/plugins/index.js'],
    rules: {
      '@typescript-eslint/no-var-requires': 'off',
      'no-undef': 'off',
    },
  },
];
