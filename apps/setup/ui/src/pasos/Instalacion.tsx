import { Button, Card, PageHeader, Progress } from '@openfactu/ui';
import { AlertOctagon, Check, Loader2 } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';

import { escucharInstalacion, instalar, type InstallEvent, type Settings } from '../api';

interface Props {
  settings: Settings;
  onTerminado: () => void;
}

interface Paso {
  titulo: string;
  estado: 'hecho' | 'actual';
}

export function Instalacion({ settings, onTerminado }: Props) {
  const [pasos, setPasos] = useState<Paso[]>([]);
  const [progreso, setProgreso] = useState(0);
  const [descarga, setDescarga] = useState<{ artefacto: string; porcentaje: number } | null>(null);
  const [registro, setRegistro] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const finRegistro = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let desuscribir: (() => void) | undefined;

    const manejar = (evento: InstallEvent) => {
      switch (evento.kind) {
        case 'step':
          setPasos((actuales) => [
            ...actuales.map((p) => ({ ...p, estado: 'hecho' as const })),
            { titulo: evento.title, estado: 'actual' },
          ]);
          setProgreso(Math.round(((evento.index - 1) / evento.total) * 100));
          setDescarga(null);
          break;
        case 'download':
          setDescarga({
            artefacto: evento.artifact,
            porcentaje: evento.total ? Math.round((evento.received / evento.total) * 100) : 0,
          });
          break;
        case 'log':
          setRegistro((lineas) => [...lineas, evento.message]);
          break;
        case 'done':
          setProgreso(100);
          setPasos((actuales) => actuales.map((p) => ({ ...p, estado: 'hecho' as const })));
          onTerminado();
          break;
        case 'error':
          setError(evento.message);
          break;
      }
    };

    escucharInstalacion(manejar).then((fn) => {
      desuscribir = fn;
    });

    instalar(settings).catch((e) => setError(String(e)));

    return () => desuscribir?.();
    // La instalación se lanza una sola vez al entrar en el paso.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    finRegistro.current?.scrollIntoView({ block: 'end' });
  }, [registro]);

  return (
    <div className="mx-auto max-w-3xl">
      <PageHeader
        title={error ? 'La instalación se ha detenido' : 'Instalando Keirost'}
        subtitle={
          error
            ? 'No se ha completado. Abajo está el detalle de lo que ocurrió.'
            : 'Puede tardar unos minutos: se descarga y se prepara todo.'
        }
      />

      <Card className="mt-6">
        <Progress
          value={progreso}
          showValue
          label={descarga ? `Descargando ${descarga.artefacto}` : 'Progreso'}
          variant={error ? 'danger' : 'accent'}
        />
        {descarga && (
          <div className="mt-3">
            <Progress value={descarga.porcentaje} size="sm" showValue />
          </div>
        )}

        <ul className="mt-5 grid gap-2 text-sm">
          {pasos.map((paso, indice) => (
            <li key={`${paso.titulo}-${indice}`} className="flex items-center gap-2">
              {paso.estado === 'hecho' ? (
                <Check className="h-4 w-4 text-accent" />
              ) : (
                <Loader2 className="h-4 w-4 animate-spin text-accent" />
              )}
              <span className={paso.estado === 'hecho' ? 'text-[var(--fg-subtle)]' : ''}>
                {paso.titulo}
              </span>
            </li>
          ))}
        </ul>
      </Card>

      {error && (
        <Card className="mt-4 border-[var(--k-danger)]" title="Qué ha fallado">
          <div className="flex items-start gap-3">
            <AlertOctagon className="mt-0.5 h-5 w-5 shrink-0 text-[var(--k-danger)]" />
            <div className="text-sm">
              <p>{error}</p>
              <p className="mt-2 text-[var(--fg-subtle)]">
                Los datos ya creados se conservan: al volver a ejecutar el instalador, se
                retoma desde donde estaba.
              </p>
            </div>
          </div>
          <Button variant="secondary" size="sm" className="mt-4" onClick={() => location.reload()}>
            Volver a empezar
          </Button>
        </Card>
      )}

      {registro.length > 0 && (
        <Card className="mt-4" title="Detalle" bodyClassName="max-h-52 overflow-y-auto">
          <pre className="whitespace-pre-wrap font-mono text-xs text-[var(--fg-subtle)]">
            {registro.join('\n')}
          </pre>
          <div ref={finRegistro} />
        </Card>
      )}
    </div>
  );
}
