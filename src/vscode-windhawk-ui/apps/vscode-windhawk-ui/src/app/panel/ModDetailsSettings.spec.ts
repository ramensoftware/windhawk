/**
 * Tests for YamlSchemaValidator and YamlConverter
 * 
 * These tests ensure:
 * 1. Round-trip conversion (settings -> YAML -> settings) works correctly
 * 2. Schema validation properly detects invalid keys and type mismatches
 * 3. Complex nested structures are handled correctly
 */

import { TextDecoder, TextEncoder } from 'util';

// Mock dependencies that are not needed for testing
jest.mock('monaco-editor', () => ({}));
jest.mock('@monaco-editor/react', () => ({
  loader: { config: jest.fn() },
}));
jest.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
jest.mock('react-router-dom', () => ({
  useBlocker: () => ({ state: 'unblocked', proceed: jest.fn(), reset: jest.fn() }),
}));
jest.mock('../webviewIPC', () => ({
  useGetModSettings: jest.fn(),
  useSetModSettings: jest.fn(),
}));
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const globalAny = global as any;
if (!globalAny.TextEncoder) {
  globalAny.TextEncoder = TextEncoder;
}
if (!globalAny.TextDecoder) {
  globalAny.TextDecoder = TextDecoder;
}

// eslint-disable-next-line import/first
import {
  exportedForTesting,
  typesForTesting,
} from './ModDetailsSettings';
// eslint-disable-next-line import/first
import * as yaml from 'js-yaml';
// eslint-disable-next-line import/first
import i18next from 'i18next';

const {
  YamlSchemaValidator,
  YamlConverter,
} = exportedForTesting;

type ModSettings = typesForTesting["ModSettings"];
type InitialSettings = typesForTesting["InitialSettings"]

// Mock translation function for tests
const mockT = ((key: string, params?: Record<string, string | number>): string => {
  if (key === 'modDetails.settings.yamlInvalid') return 'Invalid YAML structure';
  if (key === 'modDetails.settings.yamlInvalidKey') return `Invalid key: ${params?.['key']}`;
  if (key === 'modDetails.settings.yamlTypeMismatch') {
    return `Type mismatch for ${params?.['key']}: expected ${params?.['expected']}, got ${params?.['actual']}`;
  }
  if (key === 'modDetails.settings.yamlParseError') return `Parse error: ${params?.['error']}`;
  return key;
}) as typeof i18next.t;

// Helper function to parse YAML text into an object for comparison
const parseYaml = (yamlText: string): unknown => {
  if (!yamlText) return {};
  return yaml.load(yamlText);
};

// =============================================================================
// TESTS
// =============================================================================

describe('YamlConverter and YamlSchemaValidator', () => {

  // ===== Simple Types Tests =====

  describe('Simple primitive types', () => {
    const initialSettings: InitialSettings = [
      { key: 'enabled', value: true },
      { key: 'count', value: 42 },
      { key: 'name', value: 'test' },
    ];

    it('should convert flat settings to YAML and back', () => {
      const flatSettings: ModSettings = {
        enabled: 1,
        count: 100,
        name: 'mytest',
      };

      const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
      expect(yamlText).toBeTruthy();

      const validator = new YamlSchemaValidator(initialSettings);
      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      expect(result.error).toBeNull();
      expect(result.settings).toEqual(flatSettings);
    });

    it('should handle empty values correctly', () => {
      const flatSettings: ModSettings = {
        enabled: 0,
        count: 0,
        name: '',
      };

      const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
      const parsed = parseYaml(yamlText);

      // Empty values are kept in objects, only array ends are trimmed
      expect(parsed).toEqual({
        enabled: 0,
        count: 0,
        name: '',
      });
    });

    it('should detect invalid keys', () => {
      const validator = new YamlSchemaValidator(initialSettings);
      const yamlText = 'invalid_key: 123';

      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      expect(result.error).toContain('Invalid key: invalid_key');
      expect(result.settings).toBeNull();
    });

    it('should detect type mismatches', () => {
      const validator = new YamlSchemaValidator(initialSettings);
      const yamlText = 'count: "not a number"';

      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      expect(result.error).toContain('Type mismatch');
      expect(result.settings).toBeNull();
    });

    it('should coerce flat settings to schema primitive types when generating YAML', () => {
      const flatSettings: ModSettings = {
        enabled: '1',
        count: '123',
        name: 'value',
      };

      const nested = YamlConverter.flatToNested(flatSettings, initialSettings);

      expect(nested['enabled']).toBe(1);

      const countValue = nested['count'];
      expect(typeof countValue).toBe('number');
      expect(countValue).toBe(123);

      expect(nested['name']).toBe('value');
    });
  });

  // ===== Primitive Arrays Tests =====

  describe('Primitive arrays', () => {
    const initialSettings: InitialSettings = [
      { key: 'numbers', value: [1, 2, 3] },
      { key: 'strings', value: ['a', 'b', 'c'] },
    ];

    it('should handle number arrays', () => {
      const flatSettings: ModSettings = {
        'numbers[0]': 10,
        'numbers[1]': 20,
        'numbers[2]': 30,
      };

      const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
      const validator = new YamlSchemaValidator(initialSettings);
      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      expect(result.error).toBeNull();
      // Schema defines both numbers and strings arrays
      // strings gets default first entry since it's not in flatSettings
      expect(result.settings).toEqual({
        ...flatSettings,
        'strings[0]': '', // Default first entry for array in schema
      });
    });

    it('should handle string arrays', () => {
      const flatSettings: ModSettings = {
        'strings[0]': 'hello',
        'strings[1]': 'world',
      };

      const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
      const validator = new YamlSchemaValidator(initialSettings);
      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      expect(result.error).toBeNull();
      // Schema defines both numbers and strings arrays
      // numbers gets default first entry since it's not in flatSettings
      expect(result.settings).toEqual({
        'numbers[0]': 0, // Default first entry for array in schema
        ...flatSettings,
      });
    });

    it('should fill leading gaps in number arrays', () => {
      const flatSettings: ModSettings = {
        'numbers[3]': 7,
      };

      const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
      const parsed = parseYaml(yamlText);

      expect(parsed).toEqual({
        numbers: [0, 0, 0, 7],
        strings: [''],
      });

      const validator = new YamlSchemaValidator(initialSettings);
      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      expect(result.error).toBeNull();
      expect(result.settings).toEqual({
        'numbers[0]': 0,
        'numbers[1]': 0,
        'numbers[2]': 0,
        'numbers[3]': 7,
        'strings[0]': '',
      });
    });

    it('should detect wrong type in number array', () => {
      const validator = new YamlSchemaValidator(initialSettings);
      const yamlText = 'numbers:\n  - 1\n  - "wrong"\n  - 3';

      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      expect(result.error).toContain('Type mismatch');
      expect(result.settings).toBeNull();
    });
  });

  // ===== Nested Objects Tests =====

  describe('Nested objects', () => {
    const initialSettings: InitialSettings = [
      {
        key: 'database',
        value: [
          { key: 'host', value: 'localhost' },
          { key: 'port', value: 5432 },
          { key: 'enabled', value: true },
        ],
      },
    ];

    it('should handle nested object settings', () => {
      const flatSettings: ModSettings = {
        'database.host': 'example.com',
        'database.port': 3306,
        'database.enabled': 1,
      };

      const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
      const validator = new YamlSchemaValidator(initialSettings);
      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      expect(result.error).toBeNull();
      expect(result.settings).toEqual(flatSettings);
    });

    it('should detect invalid nested keys', () => {
      const validator = new YamlSchemaValidator(initialSettings);
      const yamlText = 'database:\n  host: test\n  invalid: 123';

      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      expect(result.error).toContain('Invalid key: database.invalid');
      expect(result.settings).toBeNull();
    });

    it('should respect schema order when filling defaults', () => {
      const flatSettings: ModSettings = {
        'database.port': 3306,
      };

      const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
      const lines = yamlText.split('\n');
      const hostIndex = lines.findIndex(l => l.includes('host:'));
      const portIndex = lines.findIndex(l => l.includes('port:'));
      const enabledIndex = lines.findIndex(l => l.includes('enabled:'));

      expect(hostIndex).toBeGreaterThan(-1);
      expect(portIndex).toBeGreaterThan(-1);
      expect(enabledIndex).toBeGreaterThan(-1);
      expect(hostIndex).toBeLessThan(portIndex);
      expect(portIndex).toBeLessThan(enabledIndex);

      const validator = new YamlSchemaValidator(initialSettings);
      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      expect(result.error).toBeNull();
      expect(result.settings).toEqual({
        'database.host': '',
        'database.port': 3306,
        'database.enabled': 0,
      });
    });

    it('should maintain key order from schema', () => {
      const flatSettings: ModSettings = {
        'database.enabled': 1,
        'database.host': 'test',
        'database.port': 8080,
      };

      const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
      const parsed = parseYaml(yamlText);

      // Verify the structure
      expect(parsed).toEqual({
        database: {
          host: 'test',
          port: 8080,
          enabled: 1,
        },
      });

      // Keys should appear in schema order: host, port, enabled
      const lines = yamlText.split('\n');
      const hostIndex = lines.findIndex(l => l.includes('host:'));
      const portIndex = lines.findIndex(l => l.includes('port:'));
      const enabledIndex = lines.findIndex(l => l.includes('enabled:'));

      expect(hostIndex).toBeLessThan(portIndex);
      expect(portIndex).toBeLessThan(enabledIndex);
    });
  });

  // ===== Array of Objects Tests =====

  describe('Array of objects', () => {
    const initialSettings: InitialSettings = [
      {
        key: 'servers',
        value: [
          [
            { key: 'name', value: 'server1' },
            { key: 'port', value: 8080 },
          ],
        ],
      },
    ];

    it('should handle array of objects', () => {
      const flatSettings: ModSettings = {
        'servers[0].name': 'web-server',
        'servers[0].port': 80,
        'servers[1].name': 'db-server',
        'servers[1].port': 5432,
      };

      const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
      const validator = new YamlSchemaValidator(initialSettings);
      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      expect(result.error).toBeNull();
      expect(result.settings).toEqual(flatSettings);
    });

    it('should detect invalid keys in array elements', () => {
      const validator = new YamlSchemaValidator(initialSettings);
      const yamlText = 'servers:\n  - name: test\n    port: 80\n    invalid: 123';

      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      expect(result.error).toContain('Invalid key: servers.invalid');
      expect(result.settings).toBeNull();
    });
  });

  // ===== Deeply Nested Structures Tests =====

  describe('Deeply nested structures', () => {
    const initialSettings: InitialSettings = [
      {
        key: 'config',
        value: [
          {
            key: 'profiles',
            value: [
              [
                { key: 'name', value: 'default' },
                {
                  key: 'settings',
                  value: [
                    { key: 'theme', value: 'dark' },
                    { key: 'fontSize', value: 14 },
                  ],
                },
              ],
            ],
          },
        ],
      },
    ];

    it('should handle deeply nested structures', () => {
      const flatSettings: ModSettings = {
        'config.profiles[0].name': 'production',
        'config.profiles[0].settings.theme': 'light',
        'config.profiles[0].settings.fontSize': 16,
        'config.profiles[1].name': 'development',
        'config.profiles[1].settings.theme': 'dark',
        'config.profiles[1].settings.fontSize': 12,
      };

      const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
      const validator = new YamlSchemaValidator(initialSettings);
      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      expect(result.error).toBeNull();
      expect(result.settings).toEqual(flatSettings);
    });

    it('should detect type errors in deeply nested values', () => {
      const validator = new YamlSchemaValidator(initialSettings);
      const yamlText = `config:
  profiles:
    - name: test
      settings:
        theme: light
        fontSize: "not a number"`;

      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      expect(result.error).toContain('Type mismatch');
      expect(result.settings).toBeNull();
    });
  });

  // ===== Nested Arrays Tests =====

  describe('Nested arrays (arrays of objects containing arrays)', () => {
    describe('Arrays containing primitive arrays', () => {
      const initialSettings: InitialSettings = [
        {
          key: 'groups',
          value: [
            [
              { key: 'name', value: 'team1' },
              { key: 'tags', value: ['tag1', 'tag2'] },
            ],
          ],
        },
      ];

      it('should handle array of objects with primitive arrays', () => {
        const flatSettings: ModSettings = {
          'groups[0].name': 'frontend',
          'groups[0].tags[0]': 'react',
          'groups[0].tags[1]': 'typescript',
          'groups[1].name': 'backend',
          'groups[1].tags[0]': 'node',
          'groups[1].tags[1]': 'express',
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        expect(parsed).toEqual({
          groups: [
            { name: 'frontend', tags: ['react', 'typescript'] },
            { name: 'backend', tags: ['node', 'express'] },
          ],
        });

        const validator = new YamlSchemaValidator(initialSettings);
        const result = YamlConverter.fromYaml(yamlText, validator, mockT);

        expect(result.error).toBeNull();
        expect(result.settings).toEqual(flatSettings);
      });

      it('should fill missing array indices in nested primitive arrays', () => {
        const flatSettings: ModSettings = {
          'groups[0].name': 'team',
          'groups[0].tags[0]': 'first',
          'groups[0].tags[2]': 'third', // Sparse array - missing index 1
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        expect(parsed).toEqual({
          groups: [
            { 
              name: 'team', 
              tags: ['first', '', 'third'], // Index 1 filled with default
            },
          ],
        });

        const validator = new YamlSchemaValidator(initialSettings);
        const result = YamlConverter.fromYaml(yamlText, validator, mockT);

        expect(result.error).toBeNull();
        expect(result.settings).toEqual({
          'groups[0].name': 'team',
          'groups[0].tags[0]': 'first',
          'groups[0].tags[1]': '', // Filled with default
          'groups[0].tags[2]': 'third',
        });
      });

      it('should add missing nested array when parent object exists', () => {
        const flatSettings: ModSettings = {
          'groups[0].name': 'team',
          // groups[0].tags is missing entirely
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        expect(parsed).toEqual({
          groups: [
            { 
              name: 'team', 
              tags: [''], // Default first entry added
            },
          ],
        });

        const validator = new YamlSchemaValidator(initialSettings);
        const result = YamlConverter.fromYaml(yamlText, validator, mockT);

        expect(result.error).toBeNull();
        expect(result.settings).toEqual({
          'groups[0].name': 'team',
          'groups[0].tags[0]': '', // Default added
        });
      });
    });

    describe('Arrays containing object arrays', () => {
      const initialSettings: InitialSettings = [
        {
          key: 'departments',
          value: [
            [
              { key: 'name', value: 'Engineering' },
              {
                key: 'teams',
                value: [
                  [
                    { key: 'teamName', value: 'Team A' },
                    { key: 'size', value: 5 },
                  ],
                ],
              },
            ],
          ],
        },
      ];

      it('should handle array of objects containing object arrays', () => {
        const flatSettings: ModSettings = {
          'departments[0].name': 'Engineering',
          'departments[0].teams[0].teamName': 'Frontend',
          'departments[0].teams[0].size': 8,
          'departments[0].teams[1].teamName': 'Backend',
          'departments[0].teams[1].size': 6,
          'departments[1].name': 'Sales',
          'departments[1].teams[0].teamName': 'Enterprise',
          'departments[1].teams[0].size': 4,
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        expect(parsed).toEqual({
          departments: [
            {
              name: 'Engineering',
              teams: [
                { teamName: 'Frontend', size: 8 },
                { teamName: 'Backend', size: 6 },
              ],
            },
            {
              name: 'Sales',
              teams: [
                { teamName: 'Enterprise', size: 4 },
              ],
            },
          ],
        });

        const validator = new YamlSchemaValidator(initialSettings);
        const result = YamlConverter.fromYaml(yamlText, validator, mockT);

        expect(result.error).toBeNull();
        expect(result.settings).toEqual(flatSettings);
      });

      it('should fill sparse arrays in nested object arrays', () => {
        const flatSettings: ModSettings = {
          'departments[0].name': 'Engineering',
          'departments[0].teams[0].teamName': 'Frontend',
          'departments[0].teams[0].size': 8,
          'departments[0].teams[2].teamName': 'DevOps', // Sparse - missing index 1
          'departments[0].teams[2].size': 3,
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        expect(parsed).toEqual({
          departments: [
            {
              name: 'Engineering',
              teams: [
                { teamName: 'Frontend', size: 8 },
                { teamName: '', size: 0 }, // Index 1 filled with defaults
                { teamName: 'DevOps', size: 3 },
              ],
            },
          ],
        });

        const validator = new YamlSchemaValidator(initialSettings);
        const result = YamlConverter.fromYaml(yamlText, validator, mockT);

        expect(result.error).toBeNull();
        expect(result.settings).toEqual({
          'departments[0].name': 'Engineering',
          'departments[0].teams[0].teamName': 'Frontend',
          'departments[0].teams[0].size': 8,
          'departments[0].teams[1].teamName': '', // Filled
          'departments[0].teams[1].size': 0, // Filled
          'departments[0].teams[2].teamName': 'DevOps',
          'departments[0].teams[2].size': 3,
        });
      });

      it('should fill missing properties in nested object array elements', () => {
        const flatSettings: ModSettings = {
          'departments[0].name': 'Engineering',
          'departments[0].teams[0].teamName': 'Frontend',
          // departments[0].teams[0].size is missing
          'departments[0].teams[1].size': 6,
          // departments[0].teams[1].teamName is missing
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        expect(parsed).toEqual({
          departments: [
            {
              name: 'Engineering',
              teams: [
                { teamName: 'Frontend', size: 0 }, // size filled with default
                { teamName: '', size: 6 }, // teamName filled with default
              ],
            },
          ],
        });

        const validator = new YamlSchemaValidator(initialSettings);
        const result = YamlConverter.fromYaml(yamlText, validator, mockT);

        expect(result.error).toBeNull();
        expect(result.settings).toEqual({
          'departments[0].name': 'Engineering',
          'departments[0].teams[0].teamName': 'Frontend',
          'departments[0].teams[0].size': 0, // Filled
          'departments[0].teams[1].teamName': '', // Filled
          'departments[0].teams[1].size': 6,
        });
      });

      it('should add missing nested object array when parent exists', () => {
        const flatSettings: ModSettings = {
          'departments[0].name': 'Engineering',
          // departments[0].teams is completely missing
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        expect(parsed).toEqual({
          departments: [
            {
              name: 'Engineering',
              teams: [
                { teamName: '', size: 0 }, // Default first entry
              ],
            },
          ],
        });

        const validator = new YamlSchemaValidator(initialSettings);
        const result = YamlConverter.fromYaml(yamlText, validator, mockT);

        expect(result.error).toBeNull();
        expect(result.settings).toEqual({
          'departments[0].name': 'Engineering',
          'departments[0].teams[0].teamName': '', // Default
          'departments[0].teams[0].size': 0, // Default
        });
      });

      it('should fill leading gaps in nested object arrays', () => {
        const flatSettings: ModSettings = {
          'departments[2].name': 'Support',
          'departments[2].teams[0].teamName': 'Tier1',
          'departments[2].teams[0].size': 5,
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        expect(parsed).toEqual({
          departments: [
            { name: '', teams: [{ teamName: '', size: 0 }] },
            { name: '', teams: [{ teamName: '', size: 0 }] },
            { name: 'Support', teams: [{ teamName: 'Tier1', size: 5 }] },
          ],
        });

        const validator = new YamlSchemaValidator(initialSettings);
        const result = YamlConverter.fromYaml(yamlText, validator, mockT);

        expect(result.error).toBeNull();
        expect(result.settings).toEqual({
          'departments[0].name': '',
          'departments[0].teams[0].teamName': '',
          'departments[0].teams[0].size': 0,
          'departments[1].name': '',
          'departments[1].teams[0].teamName': '',
          'departments[1].teams[0].size': 0,
          'departments[2].name': 'Support',
          'departments[2].teams[0].teamName': 'Tier1',
          'departments[2].teams[0].size': 5,
        });
      });
    });

    describe('Triple-nested arrays', () => {
      const initialSettings: InitialSettings = [
        {
          key: 'companies',
          value: [
            [
              { key: 'companyName', value: 'Acme Inc' },
              {
                key: 'divisions',
                value: [
                  [
                    { key: 'divisionName', value: 'North' },
                    {
                      key: 'projects',
                      value: [
                        [
                          { key: 'projectName', value: 'Project X' },
                          { key: 'budget', value: 1000 },
                        ],
                      ],
                    },
                  ],
                ],
              },
            ],
          ],
        },
      ];

      it('should handle triple-nested arrays', () => {
        const flatSettings: ModSettings = {
          'companies[0].companyName': 'TechCorp',
          'companies[0].divisions[0].divisionName': 'West',
          'companies[0].divisions[0].projects[0].projectName': 'Alpha',
          'companies[0].divisions[0].projects[0].budget': 5000,
          'companies[0].divisions[0].projects[1].projectName': 'Beta',
          'companies[0].divisions[0].projects[1].budget': 3000,
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        expect(parsed).toEqual({
          companies: [
            {
              companyName: 'TechCorp',
              divisions: [
                {
                  divisionName: 'West',
                  projects: [
                    { projectName: 'Alpha', budget: 5000 },
                    { projectName: 'Beta', budget: 3000 },
                  ],
                },
              ],
            },
          ],
        });

        const validator = new YamlSchemaValidator(initialSettings);
        const result = YamlConverter.fromYaml(yamlText, validator, mockT);

        expect(result.error).toBeNull();
        expect(result.settings).toEqual(flatSettings);
      });

      it('should fill sparse arrays at all nesting levels', () => {
        const flatSettings: ModSettings = {
          'companies[0].companyName': 'TechCorp',
          'companies[0].divisions[0].divisionName': 'West',
          'companies[0].divisions[0].projects[0].projectName': 'Alpha',
          'companies[0].divisions[0].projects[0].budget': 5000,
          'companies[0].divisions[0].projects[2].projectName': 'Gamma', // Sparse
          'companies[0].divisions[0].projects[2].budget': 2000,
          'companies[0].divisions[2].divisionName': 'South', // Sparse at division level
          'companies[0].divisions[2].projects[0].projectName': 'Delta',
          'companies[0].divisions[2].projects[0].budget': 1000,
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        expect(parsed).toEqual({
          companies: [
            {
              companyName: 'TechCorp',
              divisions: [
                {
                  divisionName: 'West',
                  projects: [
                    { projectName: 'Alpha', budget: 5000 },
                    { projectName: '', budget: 0 }, // Sparse index 1 filled
                    { projectName: 'Gamma', budget: 2000 },
                  ],
                },
                {
                  divisionName: '', // Sparse division index 1 filled
                  projects: [
                    { projectName: '', budget: 0 },
                  ],
                },
                {
                  divisionName: 'South',
                  projects: [
                    { projectName: 'Delta', budget: 1000 },
                  ],
                },
              ],
            },
          ],
        });

        const validator = new YamlSchemaValidator(initialSettings);
        const result = YamlConverter.fromYaml(yamlText, validator, mockT);

        expect(result.error).toBeNull();
        expect(result.settings).toEqual({
          'companies[0].companyName': 'TechCorp',
          'companies[0].divisions[0].divisionName': 'West',
          'companies[0].divisions[0].projects[0].projectName': 'Alpha',
          'companies[0].divisions[0].projects[0].budget': 5000,
          'companies[0].divisions[0].projects[1].projectName': '', // Filled
          'companies[0].divisions[0].projects[1].budget': 0, // Filled
          'companies[0].divisions[0].projects[2].projectName': 'Gamma',
          'companies[0].divisions[0].projects[2].budget': 2000,
          'companies[0].divisions[1].divisionName': '', // Filled
          'companies[0].divisions[1].projects[0].projectName': '', // Filled
          'companies[0].divisions[1].projects[0].budget': 0, // Filled
          'companies[0].divisions[2].divisionName': 'South',
          'companies[0].divisions[2].projects[0].projectName': 'Delta',
          'companies[0].divisions[2].projects[0].budget': 1000,
        });
      });
    });
  });

  // ===== Array of Arrays Tests =====
  // Note: Arrays of primitive arrays (number[][] or string[][]) are not supported by InitialSettings.
  // Only arrays of objects (InitialSettings[]) are supported for nested arrays.

  // ===== Complex Mixed Types Tests =====

  describe('Complex mixed types', () => {
    const initialSettings: InitialSettings = [
      { key: 'enabled', value: true },
      { key: 'tags', value: ['tag1', 'tag2'] },
      {
        key: 'users',
        value: [
          [
            { key: 'name', value: 'user' },
            { key: 'roles', value: ['admin', 'user'] },
            {
              key: 'permissions',
              value: [
                [
                  { key: 'resource', value: 'file' },
                  { key: 'actions', value: ['read', 'write'] },
                ],
              ],
            },
          ],
        ],
      },
    ];

    it('should handle complex mixed type structures', () => {
      const flatSettings: ModSettings = {
        enabled: 1,
        'tags[0]': 'important',
        'tags[1]': 'reviewed',
        'users[0].name': 'alice',
        'users[0].roles[0]': 'admin',
        'users[0].roles[1]': 'developer',
        'users[0].permissions[0].resource': 'database',
        'users[0].permissions[0].actions[0]': 'read',
        'users[0].permissions[0].actions[1]': 'write',
        'users[0].permissions[1].resource': 'api',
        'users[0].permissions[1].actions[0]': 'execute',
      };

      const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
      const validator = new YamlSchemaValidator(initialSettings);
      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      expect(result.error).toBeNull();
      expect(result.settings).toEqual(flatSettings);
    });
  });

  // ===== Edge Cases Tests =====

  describe('Edge cases', () => {
    it('should handle empty settings', () => {
      const initialSettings: InitialSettings = [];
      const flatSettings: ModSettings = {};

      const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
      expect(yamlText).toBe('');

      const validator = new YamlSchemaValidator(initialSettings);
      const result = YamlConverter.fromYaml('', validator, mockT);

      expect(result.error).toBeNull();
      expect(result.settings).toEqual({});
    });

    it('should handle settings with only empty values', () => {
      const initialSettings: InitialSettings = [
        { key: 'a', value: 0 },
        { key: 'b', value: '' },
      ];
      const flatSettings: ModSettings = { a: 0, b: '' };

      const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
      const parsed = parseYaml(yamlText);

      // Empty values are kept in objects
      expect(parsed).toEqual({
        a: 0,
        b: '',
      });
    });

    it('should reject invalid YAML', () => {
      const initialSettings: InitialSettings = [{ key: 'test', value: 1 }];
      const validator = new YamlSchemaValidator(initialSettings);

      const result = YamlConverter.fromYaml('invalid: [unclosed', validator, mockT);

      expect(result.error).toContain('Parse error');
      expect(result.settings).toBeNull();
    });

    it('should reject non-object YAML root', () => {
      const initialSettings: InitialSettings = [{ key: 'test', value: 1 }];
      const validator = new YamlSchemaValidator(initialSettings);

      const result = YamlConverter.fromYaml('- item1\n- item2', validator, mockT);

      expect(result.error).toContain('Invalid YAML structure');
      expect(result.settings).toBeNull();
    });

    it('should handle sparse arrays correctly', () => {
      const initialSettings: InitialSettings = [
        { key: 'items', value: [1, 2, 3] },
      ];
      const flatSettings: ModSettings = {
        'items[0]': 1,
        'items[2]': 3,
        'items[4]': 5,
      };

      const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
      const validator = new YamlSchemaValidator(initialSettings);
      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      expect(result.error).toBeNull();
      // Sparse array indices are filled with default values (0 for numbers)
      expect(result.settings).toEqual({
        'items[0]': 1,
        'items[1]': 0, // Filled with default
        'items[2]': 3,
        'items[3]': 0, // Filled with default
        'items[4]': 5,
      });
    });

    it('should preserve order with extra keys not in schema', () => {
      const initialSettings: InitialSettings = [
        { key: 'a', value: 1 },
        { key: 'b', value: 2 },
      ];
      const flatSettings: ModSettings = {
        a: 10,
        z: 999, // Extra key not in schema
        b: 20,
      };

      const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
      const parsed = parseYaml(yamlText);

      // Verify the structure
      expect(parsed).toEqual({
        a: 10,
        b: 20,
        z: 999,
      });

      // Schema keys (a, b) should come first in order
      const lines = yamlText.split('\n');
      const aIndex = lines.findIndex(l => l.startsWith('a:'));
      const bIndex = lines.findIndex(l => l.startsWith('b:'));
      const zIndex = lines.findIndex(l => l.startsWith('z:'));

      expect(aIndex).toBeLessThan(bIndex);
      expect(bIndex).toBeLessThan(zIndex);
    });

    it('should use natural sort for extra keys with numbers', () => {
      const initialSettings: InitialSettings = [
        { key: 'name', value: 'test' },
      ];

      // Extra keys with numbers - natural sort should order them correctly
      const flatSettings: ModSettings = {
        name: 'main',
        'item10': 10,
        'item2': 2,
        'item1': 1,
        'item20': 20,
        'item3': 3,
      };

      const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
      const parsed = parseYaml(yamlText);

      // Verify the structure
      expect(parsed).toEqual({
        name: 'main',
        item1: 1,
        item2: 2,
        item3: 3,
        item10: 10,
        item20: 20,
      });

      // Verify order in YAML: name (schema key) first, then extra keys in natural order
      const lines = yamlText.split('\n');
      const nameIndex = lines.findIndex(l => l.startsWith('name:'));
      const item1Index = lines.findIndex(l => l.startsWith('item1:'));
      const item2Index = lines.findIndex(l => l.startsWith('item2:'));
      const item3Index = lines.findIndex(l => l.startsWith('item3:'));
      const item10Index = lines.findIndex(l => l.startsWith('item10:'));
      const item20Index = lines.findIndex(l => l.startsWith('item20:'));

      // Schema key first
      expect(nameIndex).toBeLessThan(item1Index);

      // Extra keys in natural order (not lexicographic: item1, item10, item2, item20, item3)
      expect(item1Index).toBeLessThan(item2Index);
      expect(item2Index).toBeLessThan(item3Index);
      expect(item3Index).toBeLessThan(item10Index);
      expect(item10Index).toBeLessThan(item20Index);
    });

    it('should reject null values', () => {
      const initialSettings: InitialSettings = [
        { key: 'name', value: 'test' },
        { key: 'count', value: 42 },
      ];
      const validator = new YamlSchemaValidator(initialSettings);
      const yamlText = 'name: null\ncount: 100';

      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      expect(result.error).not.toBeNull();
      expect(result.error).toContain('Type mismatch');
      expect(result.settings).toBeNull();
    });

    it('should reject boolean arrays', () => {
      const initialSettings: InitialSettings = [
        { key: 'flags', value: [1, 2, 3] },
      ];
      const validator = new YamlSchemaValidator(initialSettings);
      const yamlText = 'flags:\n  - true\n  - false\n  - true';

      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      expect(result.error).not.toBeNull();
      expect(result.error).toContain('Type mismatch');
      expect(result.settings).toBeNull();
    });

    it('should reject nested null values', () => {
      const initialSettings: InitialSettings = [
        {
          key: 'database',
          value: [
            { key: 'host', value: 'localhost' },
            { key: 'port', value: 5432 },
          ],
        },
      ];
      const validator = new YamlSchemaValidator(initialSettings);
      const yamlText = 'database:\n  host: example.com\n  port: null';

      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      expect(result.error).not.toBeNull();
      expect(result.error).toContain('Type mismatch');
      expect(result.settings).toBeNull();
    });
  });

  // ===== Real-world Complex Example =====

  describe('Real-world example: VS Code-like settings', () => {
    const initialSettings: InitialSettings = [
      {
        key: 'workbench',
        value: [
          { key: 'colorTheme', value: 'dark' },
        ],
      },
      {
        key: 'editor',
        value: [
          { key: 'fontSize', value: 14 },
          { key: 'lineHeight', value: 20 },
        ],
      },
      {
        key: 'files',
        value: [
          {
            key: 'associations',
            value: [
              [
                { key: 'pattern', value: '*.config' },
                { key: 'language', value: 'json' },
              ],
            ],
          },
        ],
      },
      {
        key: 'search',
        value: [
          {
            key: 'exclude',
            value: ['**/node_modules', '**/dist'],
          },
        ],
      },
    ];

    it('should handle real-world settings round-trip', () => {
      const flatSettings: ModSettings = {
        'workbench.colorTheme': 'light',
        'editor.fontSize': 16,
        'editor.lineHeight': 24,
        'files.associations[0].pattern': '*.tsx',
        'files.associations[0].language': 'typescriptreact',
        'files.associations[1].pattern': '*.md',
        'files.associations[1].language': 'markdown',
        'search.exclude[0]': '**/build',
        'search.exclude[1]': '**/.git',
      };

      const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
      const parsed = parseYaml(yamlText);

      // Verify the structure
      expect(parsed).toEqual({
        workbench: {
          colorTheme: 'light',
        },
        editor: {
          fontSize: 16,
          lineHeight: 24,
        },
        files: {
          associations: [
            { pattern: '*.tsx', language: 'typescriptreact' },
            { pattern: '*.md', language: 'markdown' },
          ],
        },
        search: {
          exclude: ['**/build', '**/.git'],
        },
      });

      const validator = new YamlSchemaValidator(initialSettings);
      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      expect(result.error).toBeNull();
      expect(result.settings).toEqual(flatSettings);
    });
  });

  // ===== Array Ordering Tests =====

  describe('Array element ordering', () => {
    const initialSettings: InitialSettings = [
      {
        key: 'items',
        value: [
          [
            { key: 'x', value: 1 },
            { key: 'y', value: 2 },
            { key: 'z', value: 3 },
          ],
        ],
      },
    ];

    it('should maintain consistent order for array elements', () => {
      const flatSettings: ModSettings = {
        'items[1].z': 30,
        'items[0].y': 20,
        'items[1].x': 10,
        'items[0].z': 3,
        'items[1].y': 2,
        'items[0].x': 1,
      };

      const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
      const parsed = parseYaml(yamlText);

      // Verify the structure
      expect(parsed).toEqual({
        items: [
          { x: 1, y: 20, z: 3 },
          { x: 10, y: 2, z: 30 },
        ],
      });

      const validator = new YamlSchemaValidator(initialSettings);
      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      expect(result.error).toBeNull();
      expect(result.settings).toEqual(flatSettings);

      // Verify ordering in YAML - the keys should appear in schema order (x, y, z)
      // Keys may be quoted ('y':) or unquoted (x:)
      const xPos = Math.max(yamlText.indexOf('x:'), yamlText.indexOf("'x':"));
      const yPos = Math.max(yamlText.indexOf('y:'), yamlText.indexOf("'y':"));
      const zPos = Math.max(yamlText.indexOf('z:'), yamlText.indexOf("'z':"));

      // All keys should exist and be in order
      expect(xPos).toBeGreaterThanOrEqual(0);
      expect(yPos).toBeGreaterThanOrEqual(0);
      expect(zPos).toBeGreaterThanOrEqual(0);
      expect(xPos).toBeLessThan(yPos);
      expect(yPos).toBeLessThan(zPos);
    });

    it('should properly sort two-digit array indexes', () => {
      const initialSettings: InitialSettings = [
        {
          key: 'values',
          value: [1, 2, 3],
        },
      ];

      // Create flat settings with sequential indexes 0-11 to get two-digit indexes in result
      const flatSettings: ModSettings = {
        'values[0]': 0,
        'values[1]': 10,
        'values[2]': 20,
        'values[3]': 30,
        'values[4]': 40,
        'values[5]': 50,
        'values[6]': 60,
        'values[7]': 70,
        'values[8]': 80,
        'values[9]': 90,
        'values[10]': 100,
        'values[11]': 110,
      };

      const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
      const parsed = parseYaml(yamlText);

      // Verify the array is properly ordered with all values including two-digit indexes
      expect(parsed).toEqual({
        values: [0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110],
      });

      // Verify round-trip maintains all indexes including two-digit ones
      const validator = new YamlSchemaValidator(initialSettings);
      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      expect(result.error).toBeNull();
      expect(result.settings).toEqual(flatSettings);

      // Verify the YAML has indexes in correct numeric order
      const lines = yamlText.split('\n');
      const indexLines = lines
        .map((line, idx) => ({ line, idx }))
        .filter(({ line }) => line.match(/^\s*-\s+\d+$/))
        .map(({ line, idx }) => ({ value: parseInt(line.trim().slice(2)), lineIdx: idx }));

      // Check that line indexes are in ascending order (which means array indexes are too)
      for (let i = 1; i < indexLines.length; i++) {
        expect(indexLines[i].lineIdx).toBeGreaterThan(indexLines[i - 1].lineIdx);
        expect(indexLines[i].value).toBeGreaterThan(indexLines[i - 1].value);
      }
    });

    it('should preserve order when array starts at non-zero index', () => {
      const initialSettings: InitialSettings = [
        { key: 'w', value: 1 },
        {
          key: 'x',
          value: [
            [
              { key: 'a', value: 'test' },
              { key: 'b', value: 42 },
            ],
          ],
        },
        { key: 'y', value: [10, 20] },
        { key: 'z', value: 2 },
      ];

      // Array starts at index [1] instead of [0]
      const flatSettings: ModSettings = {
        w: 100,
        'x[1].a': 'hello',
        'x[1].b': 50,
        'x[2].a': 'world',
        'y[1]': 200,
        'y[2]': 300,
        z: 400,
      };

      const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
      const parsed = parseYaml(yamlText);

      // Verify structure (sparse arrays get filled with defaults)
      expect(parsed).toEqual({
        w: 100,
        x: [
          { a: '', b: 0 }, // Default values for missing x[0]
          { a: 'hello', b: 50 },
          { a: 'world', b: 0 }, // Default value for missing b in x[2]
        ],
        y: [0, 200, 300], // Default value for missing y[0]
        z: 400,
      });

      // Verify order in YAML: w, x, y, z (schema order preserved even with non-zero array start)
      const wPos = yamlText.indexOf('w:');
      const xPos = Math.max(yamlText.indexOf('x:'), yamlText.indexOf("'x':"));
      const yPos = Math.max(yamlText.indexOf('y:'), yamlText.indexOf("'y':"));
      const zPos = yamlText.indexOf('z:');

      expect(wPos).toBeGreaterThan(-1);
      expect(xPos).toBeGreaterThan(-1);
      expect(yPos).toBeGreaterThan(-1);
      expect(zPos).toBeGreaterThan(-1);

      // Schema order should be preserved: w, then x, then y, then z
      expect(wPos).toBeLessThan(xPos);
      expect(xPos).toBeLessThan(yPos);
      expect(yPos).toBeLessThan(zPos);
    });
  });

  // ===== Conflicting Key Types Tests =====

  describe('Conflicting key types (array vs object)', () => {
    it('should handle mixed array/object notation by treating object path as invalid', () => {
      // Schema defines 'profiles' as an array of objects
      const initialSettings: InitialSettings = [
        {
          key: 'config',
          value: [
            {
              key: 'profiles',
              value: [
                [
                  { key: 'name', value: 'default' },
                ],
              ],
            },
          ],
        },
      ];

      // Flat settings mixing array and object access
      const flatSettings: ModSettings = {
        'config.profiles[0].name': 'production',
        'config.profiles.theme': 'light', // Invalid - profiles is an array
      };

      const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
      const parsed = parseYaml(yamlText);

      // toYaml only includes valid keys based on schema
      // config.profiles.theme is not in schema, so it won't be in YAML
      expect(parsed).toEqual({
        config: {
          profiles: [
            { name: 'production' },
          ],
        },
      });

      const validator = new YamlSchemaValidator(initialSettings);
      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      // YAML only has valid structure, so validation passes
      expect(result.error).toBeNull();
      expect(result.settings).not.toBeNull();
    });

    it('should handle mixed array/object notation by treating object path as invalid', () => {
      // Schema defines 'profiles' as an array of objects
      const initialSettings: InitialSettings = [
        {
          key: 'config',
          value: [
            {
              key: 'profiles',
              value: [
                [
                  { key: 'name', value: 'default' },
                ],
              ],
            },
          ],
        },
      ];

      // Flat settings mixing array and object access
      const flatSettings: ModSettings = {
        'config.profiles.theme': 'light', // Invalid - profiles is an array
      };

      const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
      const parsed = parseYaml(yamlText);

      // toYaml only includes valid keys based on schema
      // config.profiles.theme is not in schema, so it won't be in YAML
      // But arrays in schema get default first entry
      expect(parsed).toEqual({
        config: {
          profiles: [
            { name: '' }, // Default first entry
          ],
        },
      });

      const validator = new YamlSchemaValidator(initialSettings);
      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      // YAML only has valid structure, so validation passes
      expect(result.error).toBeNull();
      expect(result.settings).not.toBeNull();
    });

    it('should handle mixed object/array notation by treating array path as invalid', () => {
      // Schema defines 'settings' as an object (not an array)
      const initialSettings: InitialSettings = [
        {
          key: 'config',
          value: [
            {
              key: 'settings',
              value: [
                { key: 'theme', value: 'dark' },
                { key: 'fontSize', value: 14 },
              ],
            },
          ],
        },
      ];

      // Flat settings mixing object and array access
      const flatSettings: ModSettings = {
        'config.settings.theme': 'light',
        'config.settings[0].name': 'invalid', // Invalid - settings is an object, not an array
      };

      const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
      const parsed = parseYaml(yamlText);

      // toYaml now filters out keys that don't match schema structure
      // config.settings[0].name is filtered because settings is an object in schema
      // Schema keys get default values (0 for fontSize since it's not in flatSettings)
      expect(parsed).toEqual({
        config: {
          settings: {
            theme: 'light',
            fontSize: 0, // Default value for schema key not in flatSettings
          },
        },
      });

      const validator = new YamlSchemaValidator(initialSettings);
      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      // YAML only has valid structure, so validation passes
      // Default values are included for all schema keys
      expect(result.error).toBeNull();
      expect(result.settings).toEqual({
        'config.settings.theme': 'light',
        'config.settings.fontSize': 0, // Default value added
      });
    });
  });

  // ===== Numeric Keys Tests =====

  describe('Numeric keys', () => {
    it('should handle numeric property keys at root level', () => {
      const initialSettings: InitialSettings = [
        { key: 'name', value: 'test' },
        { key: '100', value: 42 },
        { key: 'enabled', value: true },
      ];

      const flatSettings: ModSettings = {
        name: 'myname',
        '100': 999,
        enabled: 1,
      };

      const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
      const parsed = parseYaml(yamlText);

      // Verify the structure (numeric keys work at root level)
      expect(parsed).toEqual({
        name: 'myname',
        100: 999,
        enabled: 1,
      });

      // Note: JavaScript automatically sorts numeric keys first, so schema order cannot be preserved
      // for numeric keys. The YAML will have '100' first, then 'name', then 'enabled'.
      // This is a known limitation due to JavaScript's object key ordering rules.

      // Verify round-trip works correctly (order doesn't affect functionality)
      const validator = new YamlSchemaValidator(initialSettings);
      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      expect(result.error).toBeNull();
      expect(result.settings).toEqual(flatSettings);
    });

    it('should handle numeric keys in nested objects', () => {
      const initialSettings: InitialSettings = [
        {
          key: 'config',
          value: [
            { key: 'name', value: 'test' },
            { key: '42', value: 100 },
            { key: 'count', value: 5 },
          ],
        },
      ];

      const flatSettings: ModSettings = {
        'config.name': 'myconfig',
        'config.42': 888,
        'config.count': 10,
      };

      const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
      const parsed = parseYaml(yamlText);

      // Verify the structure - numeric property keys work in nested objects
      expect(parsed).toEqual({
        config: {
          name: 'myconfig',
          42: 888,
          count: 10,
        },
      });

      // Verify round-trip works correctly
      const validator = new YamlSchemaValidator(initialSettings);
      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      expect(result.error).toBeNull();
      expect(result.settings).toEqual(flatSettings);
    });

    it('should handle keys with numeric prefixes', () => {
      const initialSettings: InitialSettings = [
        {
          key: 'config',
          value: [
            { key: 'name', value: 'test' },
            { key: 'port80', value: 100 },
            { key: 'count', value: 5 },
          ],
        },
      ];

      const flatSettings: ModSettings = {
        'config.name': 'myconfig',
        'config.port80': 888,
        'config.count': 10,
      };

      const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
      const parsed = parseYaml(yamlText);

      // Verify the structure - keys with numeric suffixes work fine
      expect(parsed).toEqual({
        config: {
          name: 'myconfig',
          port80: 888,
          count: 10,
        },
      });

      // Verify round-trip works correctly
      const validator = new YamlSchemaValidator(initialSettings);
      const result = YamlConverter.fromYaml(yamlText, validator, mockT);

      expect(result.error).toBeNull();
      expect(result.settings).toEqual(flatSettings);
    });
  });

  describe('Empty value handling (removeEmptyValues)', () => {
    describe('Primitive empty values', () => {
      it('should keep empty strings in objects', () => {
        const initialSettings: InitialSettings = [
          { key: 'name', value: 'test' },
          { key: 'description', value: 'desc' },
        ];

        const flatSettings: ModSettings = {
          'name': 'John',
          'description': '', // Empty string kept in objects
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        expect(parsed).toEqual({
          name: 'John',
          description: '', // Empty strings are kept
        });
      });

      it('should keep zero values in objects', () => {
        const initialSettings: InitialSettings = [
          { key: 'count', value: 10 },
          { key: 'size', value: 20 },
        ];

        const flatSettings: ModSettings = {
          'count': 5,
          'size': 0, // Zero kept in objects
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        expect(parsed).toEqual({
          count: 5,
          size: 0, // Zeros are kept
        });
      });

      it('should keep non-empty values', () => {
        const initialSettings: InitialSettings = [
          { key: 'name', value: 'test' },
          { key: 'count', value: 10 },
        ];

        const flatSettings: ModSettings = {
          'name': 'John',
          'count': 42,
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        expect(parsed).toEqual({
          name: 'John',
          count: 42,
        });
      });
    });

    describe('Arrays with empty values', () => {
      it('should remove trailing empty values from primitive arrays', () => {
        const initialSettings: InitialSettings = [
          { key: 'numbers', value: [1, 2, 3] },
        ];

        const flatSettings: ModSettings = {
          'numbers[0]': 5,
          'numbers[1]': 10,
          'numbers[2]': 0, // Trailing zero should be removed
          'numbers[3]': 0, // Trailing zero should be removed
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        expect(parsed).toEqual({
          numbers: [5, 10],
        });
      });

      it('should keep empty values in the middle of arrays', () => {
        const initialSettings: InitialSettings = [
          { key: 'numbers', value: [1, 2, 3] },
        ];

        const flatSettings: ModSettings = {
          'numbers[0]': 5,
          'numbers[1]': 0, // Zero in middle should be kept
          'numbers[2]': 10,
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        expect(parsed).toEqual({
          numbers: [5, 0, 10],
        });
      });

      it('should remove trailing empty strings from string arrays', () => {
        const initialSettings: InitialSettings = [
          { key: 'tags', value: ['a', 'b'] },
        ];

        const flatSettings: ModSettings = {
          'tags[0]': 'foo',
          'tags[1]': 'bar',
          'tags[2]': '', // Trailing empty string
          'tags[3]': '', // Trailing empty string
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        expect(parsed).toEqual({
          tags: ['foo', 'bar'],
        });
      });

      it('should keep empty strings in the middle of arrays', () => {
        const initialSettings: InitialSettings = [
          { key: 'tags', value: ['a', 'b'] },
        ];

        const flatSettings: ModSettings = {
          'tags[0]': 'foo',
          'tags[1]': '', // Empty in middle
          'tags[2]': 'bar',
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        expect(parsed).toEqual({
          tags: ['foo', '', 'bar'],
        });
      });
    });

    describe('Objects with empty values', () => {
      it('should keep empty properties in nested objects', () => {
        const initialSettings: InitialSettings = [
          {
            key: 'person',
            value: [
              { key: 'name', value: 'test' },
              { key: 'age', value: 0 },
              { key: 'email', value: '' },
            ],
          },
        ];

        const flatSettings: ModSettings = {
          'person.name': 'John',
          'person.age': 0, // Kept in objects
          'person.email': '', // Kept in objects
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        expect(parsed).toEqual({
          person: {
            name: 'John',
            age: 0,
            email: '',
          },
        });
      });

      it('should keep nested objects even if they only contain empty values', () => {
        const initialSettings: InitialSettings = [
          {
            key: 'config',
            value: [
              { key: 'name', value: 'test' },
              {
                key: 'settings',
                value: [
                  { key: 'enabled', value: false },
                ],
              },
            ],
          },
        ];

        const flatSettings: ModSettings = {
          'config.name': 'test',
          'config.settings.enabled': 0, // Empty value kept in object
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        expect(parsed).toEqual({
          config: {
            name: 'test',
            settings: {
              enabled: 0, // Empty values kept in objects
            },
          },
        });
      });

      it('should keep nested objects with at least one non-empty value', () => {
        const initialSettings: InitialSettings = [
          {
            key: 'config',
            value: [
              {
                key: 'settings',
                value: [
                  { key: 'enabled', value: false },
                  { key: 'count', value: 0 },
                ],
              },
            ],
          },
        ];

        const flatSettings: ModSettings = {
          'config.settings.enabled': 1,
          'config.settings.count': 0, // Kept in objects
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        expect(parsed).toEqual({
          config: {
            settings: {
              enabled: 1,
              count: 0, // Empty values kept
            },
          },
        });
      });
    });

    describe('Arrays of objects with empty values', () => {
      it('should keep empty properties in objects in arrays', () => {
        const initialSettings: InitialSettings = [
          {
            key: 'users',
            value: [
              [
                { key: 'name', value: 'test' },
                { key: 'email', value: '' },
              ],
            ],
          },
        ];

        const flatSettings: ModSettings = {
          'users[0].name': 'Alice',
          'users[0].email': '', // Kept in objects
          'users[1].name': 'Bob',
          'users[1].email': 'bob@test.com',
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        expect(parsed).toEqual({
          users: [
            { name: 'Alice', email: '' },
            { name: 'Bob', email: 'bob@test.com' },
          ],
        });
      });

      it('should remove trailing empty objects from arrays', () => {
        const initialSettings: InitialSettings = [
          {
            key: 'items',
            value: [
              [
                { key: 'name', value: 'test' },
                { key: 'count', value: 0 },
              ],
            ],
          },
        ];

        const flatSettings: ModSettings = {
          'items[0].name': 'Item1',
          'items[0].count': 5,
          'items[1].name': '', // Empty object (all properties empty)
          'items[1].count': 0,  // Empty object (all properties empty)
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        expect(parsed).toEqual({
          items: [
            { name: 'Item1', count: 5 },
            // Empty object removed from trailing position
          ],
        });
      });

      it('should keep empty objects in the middle of arrays', () => {
        const initialSettings: InitialSettings = [
          {
            key: 'items',
            value: [
              [
                { key: 'name', value: 'test' },
              ],
            ],
          },
        ];

        const flatSettings: ModSettings = {
          'items[0].name': 'Item1',
          'items[1].name': '', // Empty object in middle
          'items[2].name': 'Item3',
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        // Empty object in middle is kept with its properties
        expect(parsed).toEqual({
          items: [
            { name: 'Item1' },
            { name: '' }, // Empty value kept in object
            { name: 'Item3' },
          ],
        });
      });
    });

    describe('Deeply nested structures with empty values', () => {
      it('should keep deeply nested empty values in objects', () => {
        const initialSettings: InitialSettings = [
          {
            key: 'root',
            value: [
              {
                key: 'level1',
                value: [
                  {
                    key: 'level2',
                    value: [
                      { key: 'value1', value: 'test' },
                      { key: 'value2', value: '' },
                    ],
                  },
                ],
              },
            ],
          },
        ];

        const flatSettings: ModSettings = {
          'root.level1.level2.value1': 'data',
          'root.level1.level2.value2': '', // Deep empty value
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        expect(parsed).toEqual({
          root: {
            level1: {
              level2: {
                value1: 'data',
                value2: '', // Empty values kept in objects
              },
            },
          },
        });
      });

      it('should keep nested objects even if they contain only empty values', () => {
        const initialSettings: InitialSettings = [
          {
            key: 'config',
            value: [
              { key: 'name', value: 'test' },
              {
                key: 'nested',
                value: [
                  { key: 'item', value: '' },
                ],
              },
            ],
          },
        ];

        const flatSettings: ModSettings = {
          'config.name': 'MyConfig',
          'config.nested.item': '', // Only value in nested is empty
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        expect(parsed).toEqual({
          config: {
            name: 'MyConfig',
            nested: {
              item: '', // Empty values kept in objects
            },
          },
        });
      });

      it('should handle arrays nested in objects nested in arrays', () => {
        const initialSettings: InitialSettings = [
          {
            key: 'groups',
            value: [
              [
                { key: 'name', value: 'test' },
                { key: 'items', value: ['a', 'b'] },
              ],
            ],
          },
        ];

        const flatSettings: ModSettings = {
          'groups[0].name': 'Group1',
          'groups[0].items[0]': 'item1',
          'groups[0].items[1]': '', // Trailing empty
          'groups[0].items[2]': '', // Trailing empty
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        expect(parsed).toEqual({
          groups: [
            {
              name: 'Group1',
              items: ['item1'], // Trailing empty values removed
            },
          ],
        });
      });
    });

    describe('Edge cases', () => {
      it('should keep completely empty structures in objects', () => {
        const initialSettings: InitialSettings = [
          { key: 'name', value: '' },
          { key: 'count', value: 0 },
        ];

        const flatSettings: ModSettings = {
          'name': '',
          'count': 0,
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        // Empty values are kept in objects
        expect(parsed).toEqual({
          name: '',
          count: 0,
        });
      });

      it('should remove array with all trailing empty values (keep first)', () => {
        const initialSettings: InitialSettings = [
          { key: 'numbers', value: [1, 2, 3] },
        ];

        const flatSettings: ModSettings = {
          'numbers[0]': 0,
          'numbers[1]': 0,
          'numbers[2]': 0,
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        // Array keeps first element only (skips index 0 when trimming)
        expect(parsed).toEqual({
          numbers: [0],
        });
      });

      it('should handle mixed empty and non-empty in complex structure', () => {
        const initialSettings: InitialSettings = [
          {
            key: 'data',
            value: [
              { key: 'active', value: true },
              { key: 'items', value: ['a', 'b'] },
              {
                key: 'config',
                value: [
                  { key: 'enabled', value: false },
                  { key: 'count', value: 0 },
                ],
              },
            ],
          },
        ];

        const flatSettings: ModSettings = {
          'data.active': 1,
          'data.items[0]': 'first',
          'data.items[1]': '', // Trailing
          'data.config.enabled': 0, // Empty
          'data.config.count': 0,   // Empty
        };

        const yamlText = YamlConverter.toYaml(flatSettings, initialSettings);
        const parsed = parseYaml(yamlText);

        expect(parsed).toEqual({
          data: {
            active: 1,
            items: ['first'], // Trailing empty removed from array
            config: { // Empty values kept in objects
              enabled: 0,
              count: 0,
            },
          },
        });
      });
    });
  });
});
