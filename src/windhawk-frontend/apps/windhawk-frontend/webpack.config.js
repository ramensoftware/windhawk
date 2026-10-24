const webpack = require('webpack');
const { NxAppWebpackPlugin } = require('@nx/webpack/app-plugin');
const { NxReactWebpackPlugin } = require('@nx/react/webpack-plugin');
const MonacoWebpackPlugin = require('monaco-editor-webpack-plugin');
const { version } = require('../../package.json');
// const BundleAnalyzerPlugin = require('webpack-bundle-analyzer').BundleAnalyzerPlugin;

// Build/serve options keyed by the active Nx configuration. These mirror the
// options that used to live in project.json under the `@nx/webpack:webpack`
// executor; the inferred `@nx/webpack/plugin` reads them from this standard
// webpack config instead.
const configValues = {
  build: {
    default: {
      compiler: 'babel',
      outputPath: '../../dist/apps/windhawk-frontend',
      index: './src/index.html',
      main: './src/main.tsx',
      polyfills: './src/polyfills.ts',
      tsConfig: './tsconfig.app.json',
      assets: [
        './src/_redirects',
        './src/favicon.ico',
        {
          glob: '**/*',
          input: './src/locales',
          output: 'locales',
          ignore: ['**/DO_NOT_EDIT.txt'],
        },
      ],
    },
    development: {
      extractLicenses: false,
      optimization: false,
      sourceMap: true,
      vendorChunk: true,
    },
    production: {
      fileReplacements: [
        {
          replace: './src/environments/environment.ts',
          with: './src/environments/environment.prod.ts',
        },
      ],
      optimization: true,
      outputHashing: 'all',
      sourceMap: false,
      namedChunks: false,
      extractLicenses: true,
      vendorChunk: false,
    },
  },
  serve: {
    default: {
      hot: true,
      liveReload: false,
      port: 4200,
      headers: { 'Access-Control-Allow-Origin': '*' },
      historyApiFallback: {
        index: '/index.html',
        disableDotRule: true,
        htmlAcceptHeaders: ['text/html', 'application/xhtml+xml'],
      },
    },
    development: {},
    production: { hot: false },
  },
};

const configuration = process.env.NX_TASK_TARGET_CONFIGURATION || 'default';

const buildOptions = {
  ...configValues.build.default,
  ...configValues.build[configuration],
};
const devServerOptions = {
  ...configValues.serve.default,
  ...configValues.serve[configuration],
};

/**
 * @type{import('webpack').WebpackOptionsNormalized}
 */
module.exports = async () => {
  // The build mode, from the BUILD_MODE environment variable. Every build is
  // either the website (windhawk.net, a plain browser) or the app: the same
  // panel with hash routing, ./locales and no Google Analytics, hosted by one
  // of two shells. 'vscode' (the default) is the windhawk-vscode webview;
  // 'tauri' is the native Windhawk UI (windhawk-core ui crate), which swaps
  // the VSCode webview transport for the Tauri bridge.
  //
  // The four flags are what the code branches on, and hold the invariant a
  // consumer may assume: WEBSITE and APP are mutually exclusive and cover
  // every build, and under APP exactly one of TAURI and VSCODE is set (under
  // WEBSITE neither is).
  const buildMode = process.env.BUILD_MODE || 'vscode';
  const isWebsite = buildMode === 'website';
  const isApp = !isWebsite;
  const isTauri = buildMode === 'tauri';
  const isVSCode = isApp && !isTauri;
  const hasMocks = configuration !== 'production';

  const plugins = [
    new NxAppWebpackPlugin(buildOptions),
    new NxReactWebpackPlugin(),

    // Inject environment variables including build mode.
    new webpack.EnvironmentPlugin({
      REACT_APP_VERSION: version,
      BUILD_MODE: buildMode,
    }),

    // Build-time constants, injected as raw values (not strings) so unused
    // branches tree-shake away.
    new webpack.DefinePlugin({
      WEBPACK_IS_WEBSITE: JSON.stringify(isWebsite),
      WEBPACK_IS_APP: JSON.stringify(isApp),
      WEBPACK_IS_TAURI: JSON.stringify(isTauri),
      WEBPACK_IS_VSCODE: JSON.stringify(isVSCode),
      WEBPACK_HAS_MOCKS: JSON.stringify(hasMocks),
    }),

    // Adjust the module rules the Nx plugins generate. This runs after
    // NxAppWebpackPlugin's apply() has populated compiler.options, so the
    // image rule exists and appended loaders land after the compiler loader.
    {
      apply(compiler) {
        const rules = compiler.options.module.rules;

        // @nx/webpack's default image rule uses `type: 'asset'`, which inlines
        // images under 10 kB as base64 `data:` URIs. The CSP allows only
        // `img-src 'self' https://...` (no `data:`), so inlined logos/icons get
        // blocked. Force images to be emitted as separate files, loaded by URL.
        const imageRule = rules.find(
          (rule) =>
            rule &&
            typeof rule === 'object' &&
            rule.test instanceof RegExp &&
            rule.test.test('.svg')
        );
        if (imageRule) {
          imageRule.type = 'asset/resource';
          delete imageRule.parser;
        }

        // Add ifdef-loader for conditional compilation.
        // This allows using /// #if WEBSITE / /// #endif directives, on the
        // same flags as the DefinePlugin constants above. It is appended last
        // so it runs before the compiler loader on raw source.
        rules.push({
          test: /\.tsx?$/,
          use: [
            {
              loader: 'ifdef-loader',
              options: {
                WEBSITE: isWebsite,
                APP: isApp,
                TAURI: isTauri,
                VSCODE: isVSCode,
                HAS_MOCKS: hasMocks,
              },
            },
          ],
        });
      },
    },
  ];

  if (isApp) {
    // Configure Monaco to only include YAML language support.
    plugins.push(
      new MonacoWebpackPlugin({
        languages: ['yaml'],
      })
    );

    // Strip the website-only <head> block (the <base href>, the website
    // CSP, and the Google Analytics scripts) delimited by the
    // <!-- windhawk.net --> markers. Both app shells serve from the app
    // root and inject their own CSP, so this block applies only to the
    // windhawk.net website build.
    plugins.push({
      apply(compiler) {
        const { Compilation, sources } = compiler.webpack;
        compiler.hooks.thisCompilation.tap('StripWebsiteBlock', (compilation) => {
          compilation.hooks.processAssets.tap(
            {
              name: 'StripWebsiteBlock',
              // After nx's index-html plugin emits index.html.
              stage: Compilation.PROCESS_ASSETS_STAGE_REPORT,
            },
            (assets) => {
              const asset = assets['index.html'];
              if (!asset) {
                return;
              }
              const html = asset
                .source()
                .toString()
                .replace(
                  /<!-- windhawk\.net -->[\s\S]*?<!-- \/windhawk\.net -->\s*/,
                  ''
                );
              compilation.updateAsset('index.html', new sources.RawSource(html));
            }
          );
        });
      },
    });
  }

  // Website mode uses browser (non-hash) routing, so unknown paths must fall
  // back to index.html.
  const devServer = isWebsite
    ? { ...devServerOptions, historyApiFallback: true }
    : devServerOptions;

  // Uncomment the following block to enable bundle size analysis.
  /*
  plugins.push(new BundleAnalyzerPlugin({
    analyzerMode: 'server',
    generateStatsFile: true,
    statsOptions: { source: false },
  }));
  */

  return {
    // Wipe stale files from the output directory before emitting. Without this
    // the different build modes (vscode/website/tauri) leave each other's
    // chunks behind, since they emit different sets of files into the same
    // dist/apps/windhawk-frontend directory.
    output: { clean: true },
    devServer,
    plugins,
  };
};
