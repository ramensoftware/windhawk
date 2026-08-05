/**
 * A pair of mod sources big enough for the source diff to be worth measuring.
 *
 * The default fixtures give the diff one line a side, which says whether it
 * renders but nothing about what it costs. Real mods reach a few thousand lines,
 * and the work of showing a diff - highlighting both files, cutting the token
 * stream into lines, marking what differs, then laying out a row per change -
 * grows with the whole file rather than with the part that changed. This is
 * ~4000 lines a side, and the changes are spread over all of it, which is the
 * shape an update usually has and the one that renders the most rows.
 *
 * Both sides are generated from the same description so the pair stays a
 * plausible before and after, and generated without randomness so a reload and a
 * test see the same diff.
 */

// Enough handlers to land near 4000 lines a side; the sizes this works out to are
// asserted in the spec, so a change to the block below has to be a deliberate one.
const HANDLER_COUNT = 152;

export type Revision = 'installed' | 'repository';

function header(version: string): string[] {
  return [
    '// ==WindhawkMod==',
    '// @id              large-diff-sample',
    '// @name            Large Diff Sample',
    '// @description     A mod large enough to measure the diff against',
    `// @version         ${version}`,
    '// @author          Mock',
    '// @github          https://github.com/mock',
    '// @include         *',
    '// @compilerOptions -lcomctl32 -lgdi32 -luser32',
    '// ==/WindhawkMod==',
    '',
    // The readme is one block comment spanning many lines, which is why
    // highlighting has to see the file whole rather than hunk by hunk.
    '// ==WindhawkModReadme==',
    '/*',
    '# Large Diff Sample',
    '',
    'This mod exists to give the diff something to chew on. It subclasses a long',
    'list of windows and logs what they do, which is enough C++ to be highlighted',
    'the way a real mod is: keywords, strings, comments, preprocessor lines and',
    'wide character literals all in the mix.',
    '',
    '## Notes',
    '',
    '- Every handler below follows the same shape, so the diff is made of many',
    '  small hunks rather than one big one.',
    '- The readme is deliberately long, so that a hunk near the top of the file',
    '  sits inside a comment that started well above it.',
    '*/',
    '// ==/WindhawkModReadme==',
    '',
    '// ==WindhawkModSettings==',
    '/*',
    '- verbose: false',
    '  $name: Verbose logging',
    '  $description: Log every message, not just the interesting ones',
    '- limit: 100',
    '  $name: Handler limit',
    '*/',
    '// ==/WindhawkModSettings==',
    '',
    '#include <windows.h>',
    '#include <windowsx.h>',
    '#include <commctrl.h>',
    '',
    'struct Settings {',
    '    bool verbose;',
    '    int limit;',
    '};',
    '',
    'static Settings g_settings;',
    '',
  ];
}

/**
 * One subclassed window: the block the body is made of, in the revision asked
 * for.
 *
 * The update stands for the kind that touches a mod all over - every log line
 * reworded - so that hunks and their context cover nearly the whole file and the
 * diff has about as many rows as the mod has lines. On top of that, every
 * eleventh block gains a case and every seventeenth loses one, so the diff
 * carries insertions and deletions and not only edits. Which blocks those are is
 * decided by the index, so the two sides can be generated apart and still line
 * up.
 */
function handler(index: number, revision: Revision): string[] {
  const updated = revision === 'repository';
  const grown = updated && index % 11 === 0;
  const trimmed = updated && index % 17 === 0;

  const lines = [
    `// Window ${index} of ${HANDLER_COUNT}.`,
    `struct Window${index}State {`,
    '    HWND hWnd;',
    '    UINT createdAt;',
    updated
      ? `    const wchar_t* name = L"window ${index} (v2)";`
      : `    const wchar_t* name = L"window ${index}";`,
    '};',
    '',
    `static Window${index}State g_window${index};`,
    `static WNDPROC g_original${index}Proc;`,
    '',
    `LRESULT CALLBACK Window${index}Proc(HWND hWnd, UINT uMsg, WPARAM wParam, LPARAM lParam) {`,
    '    switch (uMsg) {',
    '    case WM_CREATE:',
    `        g_window${index}.hWnd = hWnd;`,
    `        g_window${index}.createdAt = GetTickCount();`,
    updated
      ? `        Wh_Log(L"created %s at %u", g_window${index}.name, g_window${index}.createdAt);`
      : `        Wh_Log(L"created %s", g_window${index}.name);`,
    '        break;',
  ];

  if (grown) {
    lines.push(
      '    case WM_SIZE:',
      '        if (g_settings.verbose) {',
      `            Wh_Log(L"resized %s to %dx%d", g_window${index}.name, LOWORD(lParam), HIWORD(lParam));`,
      '        }',
      '        break;'
    );
  }

  if (!trimmed) {
    lines.push(
      '    case WM_DESTROY:',
      updated
        ? `        Wh_Log(L"destroyed %s after %u ms", g_window${index}.name, GetTickCount() - g_window${index}.createdAt);`
        : `        Wh_Log(L"destroyed %s", g_window${index}.name);`,
      `        g_window${index}.hWnd = nullptr;`,
      '        break;'
    );
  }

  lines.push(
    '    }',
    '',
    `    return CallWindowProc(g_original${index}Proc, hWnd, uMsg, wParam, lParam);`,
    '}',
    ''
  );

  return lines;
}

function footer(): string[] {
  return [
    'BOOL Wh_ModInit() {',
    '    Wh_Log(L"Init");',
    '    g_settings.verbose = Wh_GetIntSetting(L"verbose") != 0;',
    '    g_settings.limit = Wh_GetIntSetting(L"limit");',
    '    return TRUE;',
    '}',
    '',
    'void Wh_ModUninit() {',
    '    Wh_Log(L"Uninit");',
    '}',
    '',
  ];
}

function modSource(revision: Revision): string {
  const lines = [...header(revision === 'installed' ? '0.1' : '0.2')];
  for (let index = 1; index <= HANDLER_COUNT; index++) {
    lines.push(...handler(index, revision));
  }
  lines.push(...footer());
  return lines.join('\n');
}

// The source of the version on the machine, and the newer one the repository
// offers.
export const largeModSourceInstalled = modSource('installed');
export const largeModSourceRepository = modSource('repository');
