import { Button, Card, PageHeader } from '@openfactu/ui';
import {
  Activity,
  Check,
  CheckCircle2,
  Copy,
  ExternalLink,
  Globe,
  Laptop,
  Lock,
} from 'lucide-react';
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
      : `https://localhost:${settings.ports.web}`;

  // Lo que se instaló aparte del ERP. Sin esto, quien marca «analíticas» en
  // Extras termina la instalación sin saber que Grafana existe ni por dónde se
  // entra, y da por hecho que no se instaló.
  const extras: Array<{ nombre: string; detalle: string }> = [];
  if (settings.optionals.monitoring) {
    extras.push({
      nombre: 'Analíticas',
      detalle: `Grafana en http://localhost:3001 (usuario «admin», contraseña «admin» la primera vez). Los datos los recoge Prometheus, en el 9090.`,
    });
  }
  if (settings.optionals.ollama) {
    extras.push({
      nombre: 'IA local',
      detalle: 'Ollama escuchando en 127.0.0.1:11434. Los modelos se descargan al usarlos por primera vez.',
    });
  }
  if (settings.optionals.backups) {
    extras.push({
      nombre: 'Copias de seguridad',
      // Las barras van dobladas a propósito: en una cadena de JavaScript «\b»
      // es un carácter de control, no una barra, y la ruta salía rota.
      detalle:
        'Una copia diaria en C:\\ProgramData\\Keirost\\storage\\backups. La hace una tarea programada de Windows llamada «Keirost Backup».',
    });
  }

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
        <Card className="mt-4" title="Cifrado">
          <div className="flex items-start gap-3 text-sm">
            <Lock className="mt-0.5 h-5 w-5 shrink-0 text-accent" />
            <div>
              <p className="text-[var(--fg-subtle)]">
                Keirost sirve en HTTPS con un certificado propio, ya instalado como de
                confianza en este equipo.
              </p>
              <p className="mt-1 text-[var(--fg-subtle)]">
                En los demás equipos el navegador avisará hasta que instales ahí el
                certificado, que está en{' '}
                <span className="font-mono">C:\ProgramData\Keirost\config</span>.
              </p>
            </div>
          </div>
        </Card>
      )}

      {extras.length > 0 && (
        <Card className="mt-4" title="Lo que se instaló aparte">
          <div className="grid gap-4">
            {extras.map((extra) => (
              <div key={extra.nombre} className="flex items-start gap-3">
                <Activity className="mt-0.5 h-5 w-5 shrink-0 text-accent" />
                <div className="text-sm">
                  <p className="font-medium">{extra.nombre}</p>
                  <p className="text-[var(--fg-subtle)]">{extra.detalle}</p>
                </div>
              </div>
            ))}
          </div>
        </Card>
      )}

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
