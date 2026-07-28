import { Button, Card, PageHeader } from '@openfactu/ui';
import { Check, CheckCircle2, Copy, ExternalLink, Globe, Laptop } from 'lucide-react';
import { useState } from 'react';

import { abrirUrl, type Settings } from '../api';

interface Props {
  settings: Settings;
}

export function Fin({ settings }: Props) {
  const [copiado, setCopiado] = useState(false);

  // Es la única pantalla donde se ve la contraseña de la base: se genera sola y
  // luego sólo vive en el .env, que está donde no llega cualquiera.
  const conexion: Array<[string, string]> = [
    ['Servidor', `127.0.0.1:${settings.ports.database}`],
    ['Base de datos', settings.databaseName],
    ['Usuario', settings.databaseUser],
    ['Contraseña', settings.databasePassword],
  ];

  const copiar = () => {
    void navigator.clipboard
      .writeText(conexion.map(([k, v]) => `${k}: ${v}`).join('\n'))
      .then(() => {
        setCopiado(true);
        setTimeout(() => setCopiado(false), 2000);
      });
  };

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

      {settings.profile !== 'desktop' && (
        <Card
          className="mt-4"
          title="Datos de la base de datos"
          subtitle="Apúntalos: sólo se muestran aquí"
          headerAction={
            <Button variant="ghost" size="sm" onClick={copiar}>
              {copiado ? (
                <Check className="mr-2 h-4 w-4" />
              ) : (
                <Copy className="mr-2 h-4 w-4" />
              )}
              {copiado ? 'Copiado' : 'Copiar'}
            </Button>
          }
        >
          <dl className="grid gap-2 text-sm">
            {conexion.map(([etiqueta, valor]) => (
              <div key={etiqueta} className="grid grid-cols-3 gap-3">
                <dt className="text-[var(--fg-subtle)]">{etiqueta}</dt>
                <dd className="col-span-2 break-all font-mono">{valor}</dd>
              </div>
            ))}
          </dl>
          <p className="mt-3 text-xs text-[var(--fg-subtle)]">
            Keirost la usa por dentro y no hace falta para el día a día. Sirve para conectarse con
            otra herramienta o para restaurar una copia. También queda escrita en{' '}
            <span className="font-mono">{settings.dataDir}\config\.env</span>.
          </p>
        </Card>
      )}

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
