import React from 'react';
import ReactDOM from 'react-dom/client';

import { App } from './App';
import './index.css';
import { aplicar, preferenciaGuardada } from './tema';

// Antes de montar React para que no haya parpadeo al abrir la aplicación.
aplicar(preferenciaGuardada());

// La ventana navega al ERP cuando conecta; esto es lo que permite volver aquí
// desde el menú «Cambiar de servidor…».
(window as unknown as { __KEIROST_SHELL_URL__?: string }).__KEIROST_SHELL_URL__ =
  window.location.href;

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
