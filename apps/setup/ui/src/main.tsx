import React from 'react';
import ReactDOM from 'react-dom/client';

import { App } from './App';
import './index.css';
import { aplicar, preferenciaGuardada } from './tema';

// El tema se aplica antes de montar React para que no haya un parpadeo con los
// colores por defecto: se sigue lo que tenga configurado Windows salvo que el
// usuario haya elegido otra cosa.
aplicar(preferenciaGuardada());

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
