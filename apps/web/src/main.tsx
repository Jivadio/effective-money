// SPDX-License-Identifier: AGPL-3.0-or-later
import '@fontsource-variable/inter';
import './styles/tokens.css';
import './styles/app.css';
import './i18n';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App, createApp } from './App';

// Follow the system theme: DESIGN.md themes switch on `data-theme`.
const dark = window.matchMedia('(prefers-color-scheme: dark)');
const applyTheme = () => {
  document.documentElement.dataset.theme = dark.matches ? 'dark' : 'light';
};
applyTheme();
dark.addEventListener('change', applyTheme);

const root = document.getElementById('root');
if (!root) throw new Error('Missing #root element');
createRoot(root).render(
  <StrictMode>
    <App {...createApp()} />
  </StrictMode>,
);
