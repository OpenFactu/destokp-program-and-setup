import { Stepper, ToastProvider } from '@openfactu/ui';
import { useEffect, useMemo, useState } from 'react';

import {
  defaultSettings,
  detectarInstalacion,
  esAdministrador,
  rutasPorDefecto,
  type ExistingInstall,
  type Settings,
} from './api';
import { SelectorTema } from './componentes/SelectorTema';
import { AvisoInstaladorNuevo } from './componentes/AvisoInstaladorNuevo';
import { Acceso } from './pasos/Acceso';
import { Bienvenida } from './pasos/Bienvenida';
import { Credenciales } from './pasos/Credenciales';
import { Fin } from './pasos/Fin';
import { Gestor } from './pasos/Gestor';
import { Instalacion } from './pasos/Instalacion';
import { Opcionales } from './pasos/Opcionales';
import { PerfilPaso } from './pasos/Perfil';
import { Resumen } from './pasos/Resumen';
import { RutasYPuertos } from './pasos/RutasYPuertos';

/** Pasos del asistente, en orden. */
const PASOS = [
  { id: 'bienvenida', label: 'Bienvenida' },
  { id: 'perfil', label: 'Qué instalar' },
  { id: 'rutas', label: 'Rutas y puertos' },
  { id: 'credenciales', label: 'Credenciales' },
  { id: 'acceso', label: 'Acceso' },
  { id: 'opcionales', label: 'Extras' },
  { id: 'resumen', label: 'Resumen' },
  { id: 'instalacion', label: 'Instalación' },
  { id: 'fin', label: 'Listo' },
] as const;

type PasoId = (typeof PASOS)[number]['id'];

export function App() {
  const [settings, setSettings] = useState<Settings>(defaultSettings);
  const [paso, setPaso] = useState<PasoId>('bienvenida');
  const [existente, setExistente] = useState<ExistingInstall | null>(null);
  const [modoGestor, setModoGestor] = useState(false);
  const [administrador, setAdministrador] = useState(true);

  useEffect(() => {
    // Si ya hay una instalación, el asistente arranca como gestor
    // (actualizar / reparar / cambiar / desinstalar) en vez de proponer una
    // instalación nueva que machacaría la existente.
    detectarInstalacion().then((instalacion) => {
      setExistente(instalacion);
      setModoGestor(Boolean(instalacion));
    });
    esAdministrador().then(setAdministrador);
    rutasPorDefecto().then(({ programDir, dataDir }) =>
      setSettings((actual) => ({ ...actual, programDir, dataDir })),
    );
  }, []);

  const actualizar = (cambios: Partial<Settings>) =>
    setSettings((actual) => ({ ...actual, ...cambios }));

  // El perfil «sólo escritorio» no toca ni puertos ni base de datos: sus pasos
  // sobran y se saltan.
  const pasosVisibles = useMemo(
    () =>
      settings.profile === 'desktop'
        ? PASOS.filter(
            (p) => p.id !== 'rutas' && p.id !== 'opcionales' && p.id !== 'acceso',
          )
        : PASOS,
    [settings.profile],
  );

  const indice = Math.max(
    0,
    pasosVisibles.findIndex((p) => p.id === paso),
  );

  const ir = (delta: number) => {
    const siguiente = pasosVisibles[indice + delta];
    if (siguiente) setPaso(siguiente.id);
  };

  if (modoGestor && existente) {
    return (
      <ToastProvider>
        <AvisoInstaladorNuevo />
        <Gestor
          instalacion={existente}
          administrador={administrador}
          onInstalarDeNuevo={() => setModoGestor(false)}
        />
      </ToastProvider>
    );
  }

  return (
    <ToastProvider>
      <div className="flex h-full flex-col">
        <AvisoInstaladorNuevo />
        <div className="flex items-start gap-6 border-b border-[var(--border-default)] px-8 py-5">
          <Stepper
            className="flex-1"
            steps={pasosVisibles.map((p) => ({ id: p.id, label: p.label }))}
            current={indice}
            size="sm"
            onStepClick={(step) => setPaso(step.id as PasoId)}
            aria-label="Pasos de la instalación"
          />
          <SelectorTema />
        </div>

        <main className="flex-1 overflow-y-auto px-8 py-6">
          {paso === 'bienvenida' && (
            <Bienvenida administrador={administrador} onContinuar={() => ir(1)} />
          )}
          {paso === 'perfil' && (
            <PerfilPaso
              settings={settings}
              onCambiar={actualizar}
              onAtras={() => ir(-1)}
              onContinuar={() => ir(1)}
            />
          )}
          {paso === 'rutas' && (
            <RutasYPuertos
              settings={settings}
              onCambiar={actualizar}
              onAtras={() => ir(-1)}
              onContinuar={() => ir(1)}
            />
          )}
          {paso === 'credenciales' && (
            <Credenciales
              settings={settings}
              onCambiar={actualizar}
              onAtras={() => ir(-1)}
              onContinuar={() => ir(1)}
            />
          )}
          {paso === 'acceso' && (
            <Acceso
              settings={settings}
              onCambiar={actualizar}
              onAtras={() => ir(-1)}
              onContinuar={() => ir(1)}
            />
          )}
          {paso === 'opcionales' && (
            <Opcionales
              settings={settings}
              onCambiar={actualizar}
              onAtras={() => ir(-1)}
              onContinuar={() => ir(1)}
            />
          )}
          {paso === 'resumen' && (
            <Resumen
              settings={settings}
              onCambiar={actualizar}
              onAtras={() => ir(-1)}
              onInstalar={() => setPaso('instalacion')}
            />
          )}
          {paso === 'instalacion' && (
            <Instalacion settings={settings} onTerminado={() => setPaso('fin')} />
          )}
          {paso === 'fin' && <Fin settings={settings} />}
        </main>
      </div>
    </ToastProvider>
  );
}
