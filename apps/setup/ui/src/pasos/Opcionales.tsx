import { Card, PageHeader, Switch } from '@openfactu/ui';
import { Activity, BrainCircuit, HardDriveDownload } from 'lucide-react';

import type { Optionals, Settings } from '../api';
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
  const alternar = (id: keyof Optionals, valor: boolean) =>
    onCambiar({ optionals: { ...settings.optionals, [id]: valor } });

  return (
    <div className="mx-auto max-w-2xl">
      <PageHeader
        title="Extras"
        subtitle="Todo esto se puede añadir o quitar después sin reinstalar."
      />

      <div className="mt-6 grid gap-4">
        {EXTRAS.map(({ id, titulo, descripcion, icono: Icono, aviso }) => (
          <Card key={id}>
            <div className="flex items-start gap-4">
              <Icono className="mt-0.5 h-5 w-5 shrink-0 text-accent" />
              <div className="flex-1">
                <p className="font-medium">{titulo}</p>
                <p className="mt-1 text-sm text-[var(--fg-subtle)]">{descripcion}</p>
                {aviso && <p className="mt-1 text-xs text-[var(--fg-subtle)]">{aviso}</p>}
              </div>
              <Switch
                id={`extra-${id}`}
                checked={settings.optionals[id]}
                onChange={(valor) => alternar(id, valor)}
              />
            </div>
          </Card>
        ))}
      </div>

      <Navegacion onAtras={onAtras} onContinuar={onContinuar} />
    </div>
  );
}
