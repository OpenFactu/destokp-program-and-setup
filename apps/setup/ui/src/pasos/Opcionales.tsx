import { Card, PageHeader, Switch } from '@openfactu/ui';
import { Activity, BrainCircuit, HardDriveDownload } from 'lucide-react';
import { useEffect, useState } from 'react';

import { extrasPublicados, type ExtrasPublicados, type Optionals, type Settings } from '../api';
import { Navegacion } from '../componentes/Navegacion';

interface Props {
  settings: Settings;
  onCambiar: (cambios: Partial<Settings>) => void;
  onAtras: () => void;
  onContinuar: () => void;
}

const EXTRAS: Array<{
  id: keyof Optionals;
  titulo: string;
  descripcion: string;
  icono: typeof Activity;
  aviso?: string;
}> = [
  {
    id: 'backups',
    titulo: 'Copias de seguridad automáticas',
    descripcion:
      'Una copia diaria de la base de datos en la carpeta de datos, conservando las últimas.',
    icono: HardDriveDownload,
  },
  {
    id: 'ollama',
    titulo: 'IA local (Ollama)',
    descripcion:
      'Las funciones de IA de Keirost funcionan sin enviar nada fuera del equipo.',
    icono: BrainCircuit,
    aviso: 'Descarga varios GB y pide un equipo con memoria de sobra.',
  },
  {
    id: 'monitoring',
    titulo: 'Monitorización',
    descripcion: 'Prometheus, Grafana y métricas del equipo, con panel propio.',
    icono: Activity,
    aviso: 'Pensado para quien administre el servidor.',
  },
];

export function Opcionales({ settings, onCambiar, onAtras, onContinuar }: Props) {
  // Qué trae de verdad la versión que se va a instalar. Antes se ofrecían los
  // tres siempre y el que faltaba se anunciaba en un aviso perdido entre
  // veintitantos pasos de la instalación, así que quien marcaba «analíticas»
  // terminaba creyendo que las tenía.
  const [publicados, setPublicados] = useState<ExtrasPublicados | null>(null);

  useEffect(() => {
    extrasPublicados(settings.channel, settings.version)
      .then(setPublicados)
      // Sin conexión no se sabe: se dejan todos disponibles antes que esconder
      // opciones por no poder comprobarlas.
      .catch(() => setPublicados({ ollama: true, monitoring: true }));
  }, [settings.channel, settings.version]);

  const disponible = (id: keyof Optionals) => {
    if (!publicados) return true;
    // Las copias no descargan nada: las hace el pg_dump que ya se instala.
    if (id === 'backups') return true;
    return id === 'ollama' ? publicados.ollama : publicados.monitoring;
  };

  const alternar = (id: keyof Optionals, valor: boolean) =>
    onCambiar({ optionals: { ...settings.optionals, [id]: valor } });

  // Un extra marcado que la versión no publica se desmarca solo: dejarlo
  // encendido y no instalarlo es justo lo que había que arreglar.
  useEffect(() => {
    if (!publicados) return;
    const sobran = (['ollama', 'monitoring'] as const).filter(
      (id) => settings.optionals[id] && !disponible(id),
    );
    if (sobran.length === 0) return;
    const limpios = { ...settings.optionals };
    for (const id of sobran) limpios[id] = false;
    onCambiar({ optionals: limpios });
  }, [publicados]);

  return (
    <div className="mx-auto max-w-2xl">
      <PageHeader
        title="Extras"
        subtitle="Todo esto se puede añadir o quitar después sin reinstalar."
      />

      <div className="mt-6 grid gap-4">
        {EXTRAS.map(({ id, titulo, descripcion, icono: Icono, aviso }) => {
          const hay = disponible(id);
          return (
            <Card key={id} className={hay ? undefined : 'opacity-60'}>
              <div className="flex items-start gap-4">
                <Icono className="mt-0.5 h-5 w-5 shrink-0 text-accent" />
                <div className="flex-1">
                  <p className="font-medium">{titulo}</p>
                  <p className="mt-1 text-sm text-[var(--fg-subtle)]">{descripcion}</p>
                  {hay && aviso && (
                    <p className="mt-1 text-xs text-[var(--fg-subtle)]">{aviso}</p>
                  )}
                  {!hay && (
                    <p className="mt-1 text-xs text-[var(--fg-subtle)]">
                      La versión que vas a instalar no incluye esto. Se puede añadir más
                      adelante, desde el propio asistente, con una versión que lo traiga.
                    </p>
                  )}
                </div>
                <Switch
                  id={`extra-${id}`}
                  checked={settings.optionals[id] && hay}
                  disabled={!hay}
                  onChange={(valor) => alternar(id, valor)}
                />
              </div>
            </Card>
          );
        })}
      </div>

      <Navegacion onAtras={onAtras} onContinuar={onContinuar} />
    </div>
  );
}
