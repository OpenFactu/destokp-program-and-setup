import { Badge, Button, Card, PageHeader } from '@openfactu/ui';
import { AlertTriangle, Download } from 'lucide-react';
import { useEffect, useState } from 'react';

import { comprobarRequisitos, consultarVersion, type ManifestSummary, type Settings } from '../api';
import { Navegacion } from '../componentes/Navegacion';

interface Props {
  settings: Settings;
  onAtras: () => void;
  onInstalar: () => void;
}

const PERFILES: Record<Settings['profile'], string> = {
  full: 'Completo (base de datos, servidor, web y aplicación)',
  server: 'Sólo servidor (base de datos, servidor y web)',
  desktop: 'Sólo aplicación de escritorio',
};

const megas = (bytes: number) => `${Math.round(bytes / 1_048_576)} MB`;

export function Resumen({ settings, onAtras, onInstalar }: Props) {
  const [version, setVersion] = useState<ManifestSummary | null>(null);
  const [avisos, setAvisos] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [cargando, setCargando] = useState(true);

  useEffect(() => {
    setCargando(true);
    Promise.all([consultarVersion(settings.channel), comprobarRequisitos(settings)])
      .then(([manifest, requisitos]) => {
        setVersion(manifest);
        setAvisos(requisitos);
        setError(null);
      })
      .catch((e) => setError(String(e)))
      .finally(() => setCargando(false));
  }, [settings]);

  const filas: Array<[string, string]> = [
    ['Qué se instala', PERFILES[settings.profile]],
    ...(settings.profile === 'desktop'
      ? ([['Servidor', settings.remoteServer ?? '—']] as Array<[string, string]>)
      : ([
          ['Programa', settings.programDir ?? '—'],
          ['Datos', settings.dataDir ?? '—'],
          [
            'Puertos',
            `web ${settings.ports.web} · servidor ${settings.ports.server} · base de datos ${settings.ports.database}`,
          ],
          [
            'Extras',
            [
              settings.optionals.backups && 'copias',
              settings.optionals.ollama && 'IA local',
              settings.optionals.monitoring && 'monitorización',
            ]
              .filter(Boolean)
              .join(', ') || 'ninguno',
          ],
        ] as Array<[string, string]>)),
  ];

  return (
    <div className="mx-auto max-w-2xl">
      <PageHeader
        title="Todo listo"
        subtitle="Repasa antes de empezar; a partir de aquí el instalador trabaja solo."
      />

      <Card
        className="mt-6"
        title="Resumen"
        headerAction={
          version && <Badge variant="accent">Keirost {version.version}</Badge>
        }
        isLoading={cargando}
      >
        <dl className="grid gap-3">
          {filas.map(([etiqueta, valor]) => (
            <div key={etiqueta} className="grid grid-cols-3 gap-3 text-sm">
              <dt className="text-[var(--fg-subtle)]">{etiqueta}</dt>
              <dd className="col-span-2 break-words">{valor}</dd>
            </div>
          ))}
          {version && version.downloadSize > 0 && (
            <div className="grid grid-cols-3 gap-3 text-sm">
              <dt className="text-[var(--fg-subtle)]">Descarga</dt>
              <dd className="col-span-2 flex items-center gap-2">
                <Download className="h-4 w-4 text-accent" />
                {megas(version.downloadSize)}
              </dd>
            </div>
          )}
        </dl>
      </Card>

      {avisos.length > 0 && (
        <Card className="mt-4 border-[var(--k-warning)]" title="Antes de continuar">
          <ul className="grid gap-2 text-sm">
            {avisos.map((aviso) => (
              <li key={aviso} className="flex items-start gap-2">
                <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-[var(--k-warning)]" />
                <span>{aviso}</span>
              </li>
            ))}
          </ul>
        </Card>
      )}

      {error && (
        // Sin manifest no hay nada que instalar: el caso típico es un equipo
        // sin salida a internet o con un proxy por medio.
        <Card className="mt-4 border-[var(--k-danger)]" title="No se pudo consultar la versión">
          <p className="text-sm text-[var(--fg-subtle)]">{error}</p>
          <Button
            variant="secondary"
            size="sm"
            className="mt-3"
            onClick={() => consultarVersion(settings.channel).then(setVersion).catch(() => {})}
          >
            Reintentar
          </Button>
        </Card>
      )}

      <Navegacion
        onAtras={onAtras}
        onContinuar={onInstalar}
        textoContinuar="Instalar"
        continuarDeshabilitado={cargando || Boolean(error)}
        motivo={error ? 'No se pudo consultar la versión' : 'Comprobando…'}
      />
    </div>
  );
}
