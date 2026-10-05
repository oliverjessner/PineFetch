import js from '@eslint/js';
import { defineConfig, globalIgnores } from 'eslint/config';
import globals from 'globals';

export default defineConfig([
    globalIgnores([
        '**/node_modules/**',
        '**/target/**',
        'src-tauri/gen/**',
        'src-tauri/resources/**',
        'src-tauri/vendor/glib/**',
        'src/vendor/**',
        'dist/**',
        'build/**',
        '.next/**',
        'coverage/**',
    ]),
    {
        files: ['**/*.{js,mjs,cjs}'],
        extends: [js.configs.recommended],
        linterOptions: { reportUnusedDisableDirectives: 'error' },
        rules: {
            'no-constant-binary-expression': 'error',
        },
    },
    {
        files: ['src/**/*.js'],
        languageOptions: { globals: globals.browser },
    },
    {
        files: ['src/**/*.js'],
        ignores: ['src/tauri-client.js'],
        rules: {
            'no-restricted-properties': [
                'error',
                {
                    property: '__TAURI__',
                    message: 'Use the injected API client from tauri-client.js.',
                },
            ],
        },
    },
    {
        files: ['scripts/**/*.mjs', '*.config.mjs'],
        languageOptions: { globals: globals.nodeBuiltin },
    },
    {
        files: ['test/**/*.mjs'],
        languageOptions: { globals: globals.nodeBuiltin },
    },
]);
