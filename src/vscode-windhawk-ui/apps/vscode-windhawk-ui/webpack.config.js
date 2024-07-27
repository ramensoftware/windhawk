// Reference:
// https://github.com/ideafast/ideafast-portal/blob/59fb91104db81a86fc282491ac936d49fb4ef0e8/packages/itmat-ui-react/webpack.config.js#L7

const webpack = require('webpack');
const { composePlugins, withNx } = require('@nx/webpack');
const { withReact } = require('@nx/react');
const { version } = require('../../package.json');

module.exports = composePlugins(
    withNx(),
    withReact(),
    (config) => {
        config.plugins.splice(0, 0, new webpack.EnvironmentPlugin({
          REACT_APP_VERSION: version,
        }));

        return config;
    }
);
