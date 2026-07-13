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
	// VSCode-independence and contract-seam import rules. Flat-config rule
	// entries override (not merge) across objects, so each file group below
	// gets exactly one no-restricted-imports definition:
	// - shared core (services/, storage/), core client (coreClient/), and
	//   tests: no vscode. Surface user-facing messages via the Logger
	//   interface in services/logger.ts.
	// - extension layer (src/*.ts, src/utils/): no direct services/storage
	//   imports; all core access goes through src/coreClient (the WindhawkCore
	//   contract of the native core rewrite). vscode is its home turf.
	// - CLI: both restrictions (plain Node binary, contract consumer).
	// src/coreClient/ may import services/storage (the in-process backend
	// delegates to them); src/test/ may too (it injects fake services).
	{
		files: [
			'src/services/**/*.{ts,tsx}',
			'src/storage/**/*.{ts,tsx}',
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
							message: 'services/, storage/, coreClient/, and test/ must not depend on vscode. Use the Logger interface for notifications.',
						},
					],
				},
			],
		},
	},
	{
		files: ['src/*.{ts,tsx}', 'src/utils/**/*.{ts,tsx}'],
		rules: {
			'no-restricted-imports': [
				'error',
				{
					patterns: [
						{
							group: ['**/services', '**/services/**', '**/storage', '**/storage/**'],
							message: 'Front-end code must access the core through src/coreClient (the WindhawkCore contract), not services/ or storage/ directly.',
						},
					],
				},
			],
		},
	},
	{
		files: ['src/cli/**/*.{ts,tsx}'],
		rules: {
			'no-restricted-imports': [
				'error',
				{
					paths: [
						{
							name: 'vscode',
							message: 'cli/ must not depend on vscode; it runs as a plain Node binary.',
						},
					],
					patterns: [
						{
							group: ['**/services', '**/services/**', '**/storage', '**/storage/**'],
							message: 'CLI code must access the core through src/coreClient (the WindhawkCore contract), not services/ or storage/ directly.',
						},
					],
				},
			],
		},
	},
	{
		// Shared data shapes (the IPC contract with the React webview, and the
		// CLI's consumed types) must live in src/services/types.ts so there is
		// one place to keep in sync. Banning `export type X = ...` elsewhere in
		// services/ catches accidental reintroduction. Interfaces remain allowed
		// since they typically describe service contracts (e.g. Logger,
		// ModConfigService), not data shapes.
		//
		// index.ts is exempt because it exports ServicesOptions/Services types
		// that reference concrete service classes - moving them to types.ts
		// would force types.ts to import runtime modules.
		// errors.ts is exempt because ErrorCode is an internal error-taxonomy
		// tag, not an IPC data shape, and has nothing to do with the webview.
		files: ['src/services/**/*.{ts,tsx}'],
		ignores: ['src/services/types.ts', 'src/services/index.ts', 'src/services/errors.ts'],
		rules: {
			'no-restricted-syntax': [
				'error',
				{
					selector: 'ExportNamedDeclaration > TSTypeAliasDeclaration',
					message: 'Data shape types must live in src/services/types.ts so the IPC contract stays in one place. If this is a service contract, use `export interface` instead.',
				},
			],
		},
	},
]);
