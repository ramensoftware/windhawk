// @ts-check
import js from '@eslint/js';
import stylistic from '@stylistic/eslint-plugin';
import { defineConfig } from 'eslint/config';
import tseslint from 'typescript-eslint';

export default defineConfig([
	{
		ignores: ['out/', 'dist/', 'prebuilds/', 'webview/'],
	},
	{
		files: ['**/*.{ts,tsx}'],
		extends: [
			js.configs.recommended,
			tseslint.configs.recommended,
			// tseslint.configs.stylistic is intentionally not included — it
			// brings `consistent-type-definitions` (type → interface), which
			// breaks code that relies on `type` aliases being structural
			// (e.g. assignability to `Record<string, unknown>`). Other style
			// rules it enables are low-value for this codebase.
		],
		plugins: {
			'@stylistic': stylistic,
		},
		rules: {
			// 'multi-line' keeps single-line `if (x) return;` legal but still
			// requires braces around multi-line bodies (dangling-else safety).
			'curly': ['warn', 'multi-line'],
			'@stylistic/semi': ['warn', 'always'],
			'@typescript-eslint/no-empty-function': 'off',
			'@typescript-eslint/no-explicit-any': 'off',
			'@typescript-eslint/naming-convention': [
				'warn',
				{
					selector: 'import',
					format: ['camelCase', 'PascalCase'],
					// Allow namespace imports to match the module's own name
					// when that name is snake_case (e.g. `child_process`).
					filter: { regex: '^child_process$', match: false },
				},
			],
			// Limit to local variable declarations: don't flag unused function
			// args (IPC handlers receive a `message` param uniformly whether
			// used or not) or unused catch clauses (`catch (e) { /* ignore */ }`
			// is an intentional pattern here).
			'@typescript-eslint/no-unused-vars': [
				'error',
				{
					args: 'none',
					caughtErrors: 'none',
					ignoreRestSiblings: true,
				},
			],
		},
	},
	// VSCode-independence rule: the core client (coreClient/) and the tests are
	// the shared, transport-agnostic layer and must not import vscode. They
	// surface user-facing messages through the Logger interface
	// (coreClient/logger.ts) instead. The extension layer (src/*.ts,
	// src/utils/) reaches the shared core only through src/coreClient (the
	// WindhawkCore contract); vscode is its home turf.
	{
		files: [
			'src/coreClient/**/*.{ts,tsx}',
			'src/test/**/*.{ts,tsx}',
		],
		rules: {
			'no-restricted-imports': [
				'error',
				{
					paths: [
						{
							name: 'vscode',
							message: 'coreClient/ and test/ must not depend on vscode. Use the Logger interface for notifications.',
						},
					],
				},
			],
		},
	},
]);
