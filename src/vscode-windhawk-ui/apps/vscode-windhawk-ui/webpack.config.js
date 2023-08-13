// Override webpack configuration:
// https://github.com/nrwl/nx/issues/3175#issuecomment-644296842

const webpack = require('webpack');

// Require the main @nrwl/react/plugins/webpack configuration function.
const nrwlConfig = require('@nrwl/react/plugins/webpack.js');

const package = require('../../package.json');

module.exports = (config, context) => {
  // First call it so that it @nrwl/react plugin adds its configs.
  nrwlConfig(config, context);

  // then override your config.
  return {
    ...config,
    plugins: [
      ...config.plugins,
      new webpack.DefinePlugin({
        'process.env.REACT_APP_VERSION': JSON.stringify(package.version),
      }),
    ]
  };
};
