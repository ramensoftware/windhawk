/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

//@ts-check

'use strict';

const path = require('path');
const CopyWebpackPlugin = require('copy-webpack-plugin');

/**@type {import('webpack').Configuration}*/
const config = {
    target: 'node', // vscode extensions run in a Node.js-context 📖 -> https://webpack.js.org/configuration/node/

    entry: {
        // The VSCode extension - runs inside VSCode's Electron host.
        extension: './src/extension.ts',
        // The windhawk-cli entry - runs as Node (either system node or
        // Electron with ELECTRON_RUN_AS_NODE=1). Same webpack config so
        // both bundles stay in sync with a single build.
        cli: './src/cli/index.ts',
    },
    output: { // the bundle is stored in the 'dist' folder (check package.json), 📖 -> https://webpack.js.org/configuration/output/
        path: path.resolve(__dirname, 'dist'),
        filename: '[name].js',
        libraryTarget: "commonjs2",
        devtoolModuleFilenameTemplate: "../[resource-path]",
    },
    devtool: 'source-map',
    externals: {
        vscode: "commonjs vscode" // the vscode-module is created on-the-fly and must be excluded. Add other modules that cannot be webpack'ed, 📖 -> https://webpack.js.org/configuration/externals/
    },
    resolve: { // support reading TypeScript and JavaScript files, 📖 -> https://github.com/TypeStrong/ts-loader
        extensions: ['.ts', '.js'],
        alias: {
            // Workaround for https://github.com/baudehlo/node-fs-ext/pull/104
            './build/Release/fs-ext': './build/Release/fs-ext.node'
        }
    },
    module: {
        rules: [{
            test: /\.ts$/,
            exclude: /node_modules/,
            use: [{
                loader: 'ts-loader',
                options: {
                    compilerOptions: {
                        // Override `tsconfig.json` so TypeScript emits native
                        // JS modules (lets webpack tree-shake). es2020 rather
                        // than es6 so dynamic `import()` calls - used in
                        // src/cli/ to gate native-module loads behind the first
                        // command execution - compile cleanly.
                        "module": "es2020"
                    }
                }
            }]
        },
        {
            test: /\.node$/,
            loader: "node-loader",
        }]
    },
    plugins: [
        new CopyWebpackPlugin({
            patterns: [
                { from: 'node_modules/native-reg/prebuilds', to: '../prebuilds' }
            ]
        })
    ]
};

module.exports = config;
