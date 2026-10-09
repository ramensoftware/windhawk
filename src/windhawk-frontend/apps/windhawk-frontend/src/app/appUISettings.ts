import React from 'react';
import { type AppUISettings, type CurrentUser } from './webviewIPCMessages';

export type AppUISettingsContextType = Partial<AppUISettings>;

export const AppUISettingsContext =
  React.createContext<AppUISettingsContextType>({});

// The Windows account the host acts for, which a user configuration of a mod
// belongs to. Absent where the host did not say.
export const CurrentUserContext = React.createContext<CurrentUser | undefined>(
  undefined
);
