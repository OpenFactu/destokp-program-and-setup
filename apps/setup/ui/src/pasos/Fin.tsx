import { Button, Card, PageHeader } from '@openfactu/ui';
import { CheckCircle2, ExternalLink, Globe, Laptop } from 'lucide-react';

import { abrirUrl, type Settings } from '../api';

interface Props {
  settings: Settings;
}

export function Fin({ settings }: Props) {
  const urlLocal =
    settings.profile === 'desktop'
      ? (settings.remoteServer ?? '')
      : `http://localhost:${settings.ports.web}`;

  return (
    <div className="mx-auto max-w-2xl">
      <PageHeader
        icon={<CheckCircle2 className="h-7 w-7" />}
        title="Keirost está listo"
        subtitle="Arranca solo con el equipo; no hay que hacer nada más."
      />

      <Card className="mt-6" title="Cómo entrar">
        <div className="grid gap-4">
          <div className="flex items-start gap-3">
            <Laptop className="mt-0.5 h-5 w-5 shrink-0 text-accent" />
            <div className="text-sm">
              <p className="font-medium">Desde este equipo</p>
              <p className="text-[var(--fg-subtle)]">
                Con la aplicación «Keirost» del menú de inicio, o en {urlLocal}
              </p>
            </div>
          </div>

          {settings.profile !== 'desktop' && (
            <div className="flex items-start gap-3">
              <Globe className="mt-0.5 h-5 w-5 shrink-0 text-accent" />
              <div className="text-sm">
                <p className="font-medium">Desde el resto de la oficina</p>
                <p className="text-[var(--fg-subtle)]">
                  Con la dirección IP de este equipo y el puerto {settings.ports.web}. También
                  desde el móvil, en la misma red.
                </p>
              </div>
            </div>
          )}
        </div>

        <div className="mt-5 flex gap-3">
          <Button onClick={() => abrirUrl(urlLocal)}>
            Abrir Keirost
            <ExternalLink className="ml-2 h-4 w-4" />
          </Button>
        </div>
      </Card>

      <Card className="mt-4" title="Por si acaso">
        <ul className="grid gap-2 text-sm text-[var(--fg-subtle)]">
          <li>
            El usuario es <span className="font-mono">admin</span>, con la contraseña que
            acabas de elegir.
          </li>
          <li>
            Para actualizar, reparar o desinstalar, vuelve a abrir Keirost Setup: detecta la
            instalación y ofrece esas opciones.
          </li>
          {settings.profile !== 'desktop' && (
            <li>
              Los registros de los servicios están en{' '}
              <span className="font-mono">{settings.dataDir}\logs</span>.
            </li>
          )}
        </ul>
      </Card>
    </div>
  );
}
