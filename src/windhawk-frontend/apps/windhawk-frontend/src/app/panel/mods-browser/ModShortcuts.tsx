import { getDisplayModId, isLocalModId } from '@app/utils';
import { Button, Card, Select, Space } from 'antd';
import { useState } from 'react';
import styled from 'styled-components';
import type { InstalledModEntry } from '../shared/installedMod';

const storageKey = 'windhawk.home.recentMods';
const recentLimit = 3;

const Shortcuts = styled.div`
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  margin-bottom: 12px;

  > * {
    flex: 1 1 240px;
    min-width: 0;
  }
`;

function readRecentMods(): string[] {
  try {
    const stored: unknown = JSON.parse(localStorage.getItem(storageKey) ?? '[]');
    return Array.isArray(stored)
      ? stored.filter((id): id is string => typeof id === 'string')
      : [];
  } catch {
    return [];
  }
}

export function rememberViewedMod(id: string) {
  const recent = readRecentMods();
  if (recent[0] === id) return;

  const next = [id, ...recent.filter((previousId) => previousId !== id)];
  try {
    localStorage.setItem(storageKey, JSON.stringify(next.slice(0, recentLimit)));
  } catch {
    // Viewing mods still works when browser storage is unavailable.
  }
}

export function ModShortcuts({ mods, onOpen }: {
  mods: Record<string, InstalledModEntry>;
  onOpen: (id: string) => void;
}) {
  const [recent] = useState(readRecentMods);
  const recentMods = recent.filter((id) => Object.hasOwn(mods, id));
  const localOptions = Object.entries(mods)
    .filter(([id]) => isLocalModId(id))
    .map(([id, mod]) => ({
      value: id,
      label: mod.metadata?.name || getDisplayModId(id),
    }));

  if (!recentMods.length && !localOptions.length) return null;

  return (
    <Shortcuts>
      {recentMods.length > 0 && (
        <Card size="small" title="Recently viewed">
          <Space wrap>
            {recentMods.map((id) => (
              <Button type="link" key={id} onClick={() => onOpen(id)}>
                {mods[id].metadata?.name || getDisplayModId(id)}
              </Button>
            ))}
          </Space>
        </Card>
      )}
      {localOptions.length > 0 && (
        <Card size="small" title="Local mods">
          <Select
            aria-label="Open a local mod"
            placeholder="Open a local mod"
            style={{ width: '100%' }}
            showSearch={{ optionFilterProp: 'label' }}
            options={localOptions}
            onChange={onOpen}
          />
        </Card>
      )}
    </Shortcuts>
  );
}
