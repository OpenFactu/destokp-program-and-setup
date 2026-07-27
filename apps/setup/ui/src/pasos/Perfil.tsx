import { Badge, Card, Input, PageHeader } from '@openfactu/ui';
import { Check, Laptop, Network, Server } from 'lucide-react';

import type { Profile, Settings } from '../api';
import { Navegacion } from '../componentes/Navegacion';

interface Props {
  settings: Settings;
  onCambiar: (cambios: Partial<Settings>) => void;
  onAtras: () => void;
  onContinuar: () => void;
}

const PERFILES: Array<{
  id: Profile;
  titulo: string;
  descripcion: string;
  detalle: string;
  icono: typeof Server;
  recomendado?: boolean;
}> = [
  {
    id: 'full',
    titulo: 'Completo',
    descripcion: 'Todo en este equipo',
    detalle:
      'Base de datos, servidor, web y aplicación de escritorio. Es lo habitual en el ordenador principal de la empresa.',
    icono: Network,
    recomendado: true,
  },
  {
    id: 'server',
    titulo: 'Sólo servidor',
    descripcion: 'Sin aplicación de escritorio',
    detalle:
      'Para el equipo que hace de servidor. El resto de la oficina entra por el navegador o instalando sólo la aplicación.',
    icono: Server,
  },
  {
    id: 'desktop',
    titulo: 'Sólo aplicación',
    descripcion: 'Conectar a un Keirost existente',
    detalle:
      'No instala base de datos ni servidor: la aplicación se conecta al Keirost que ya tenga la empresa.',
    icono: Laptop,
  },
];

export function PerfilPaso({ settings, onCambiar, onAtras, onContinuar }: Props) {
  const necesitaServidor = settings.profile === 'desktop';
  const servidorRemoto = settings.remoteServer ?? '';
  const puedeContinuar = !necesitaServidor || servidorRemoto.trim().length > 0;

  return (
    <div className="mx-auto max-w-3xl">
      <PageHeader
        title="¿Qué quieres instalar?"
        subtitle="Se puede cambiar más adelante volviendo a abrir este instalador."
      />

      <div className="mt-6 grid gap-4">
        {PERFILES.map((perfil) => {
          const seleccionado = settings.profile === perfil.id;
          const Icono = perfil.icono;
          return (
            <button
              key={perfil.id}
              type="button"
              onClick={() => onCambiar({ profile: perfil.id })}
              className="text-left"
              aria-pressed={seleccionado}
            >
              <Card
                className={
                  seleccionado
                    ? 'border-accent ring-2 ring-[rgb(var(--color-accent-rgb)/0.2)]'
                    : 'hover:border-[var(--border-strong)]'
                }
              >
                <div className="flex items-start gap-4">
                  <Icono
                    className={`mt-0.5 h-6 w-6 shrink-0 ${
                      seleccionado ? 'text-accent' : 'text-[var(--fg-subtle)]'
                    }`}
                  />
                  <div className="flex-1">
                    <div className="flex items-center gap-2">
                      <span className="font-medium">{perfil.titulo}</span>
                      {perfil.recomendado && <Badge variant="accent">Recomendado</Badge>}
                      {seleccionado && <Check className="h-4 w-4 text-accent" />}
                    </div>
                    <p className="text-sm text-[var(--fg-subtle)]">{perfil.descripcion}</p>
                    <p className="mt-2 text-sm text-[var(--fg-subtle)]">{perfil.detalle}</p>
                  </div>
                </div>
              </Card>
            </button>
          );
        })}
      </div>

      {necesitaServidor && (
        <Card className="mt-5" title="¿Dónde está Keirost?">
          <Input
            label="Dirección del servidor"
            placeholder="http://192.168.1.50:8080"
            value={servidorRemoto}
            onChange={(e) => onCambiar({ remoteServer: e.target.value })}
            helperText="La misma dirección que usáis en el navegador para entrar en Keirost."
          />
        </Card>
      )}

      <Navegacion
        onAtras={onAtras}
        onContinuar={onContinuar}
        continuarDeshabilitado={!puedeContinuar}
        motivo="Indica la dirección del servidor"
      />
    </div>
  );
}
