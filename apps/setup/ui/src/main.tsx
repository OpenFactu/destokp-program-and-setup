import { applyTheme } from '@openfactu/ui';
import React from 'react';
import ReactDOM from 'react-dom/client';

import { App } from './App';
import './index.css';

// El tema se aplica antes de montar React para que no haya un parpadeo con los
// colores por defecto. Es el preset «Keirost Clásico» del sistema de diseño.
applyTheme({ mode: 'light', colors: { primary: '#0A1628', accent: '#0D9488' } });

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
