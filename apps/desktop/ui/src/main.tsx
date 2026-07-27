import { applyTheme } from '@openfactu/ui';
import React from 'react';
import ReactDOM from 'react-dom/client';

import { App } from './App';
import './index.css';

applyTheme({ mode: 'light', colors: { primary: '#0A1628', accent: '#0D9488' } });

// La ventana navega al ERP cuando conecta; esto es lo que permite volver aquí
// desde el menú «Cambiar de servidor…».
(window as unknown as { __KEIROST_SHELL_URL__?: string }).__KEIROST_SHELL_URL__ =
  window.location.href;

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
