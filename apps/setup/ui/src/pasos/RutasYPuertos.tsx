import { Button, Card, Input, PageHeader } from '@openfactu/ui';
import { useEffect, useState } from 'react';

import { comprobarPuerto, sugerirPuerto, type Ports, type Settings } from '../api';
import { Navegacion } from '../componentes/Navegacion';

interface Props {
  settings: Settings;
  onCambiar: (cambios: Partial<Settings>) => void;
  onAtras: () => void;
  onContinuar: () => void;
}

const PUERTOS: Array<{ campo: keyof Ports; etiqueta: string; ayuda: string }> = [
  { campo: 'web', etiqueta: 'Web', ayuda: 'Por aquí entra la gente desde el navegador.' },
  { campo: 'server', etiqueta: 'Servidor (API)', ayuda: 'Lo usa la aplicación por dentro.' },
  {
    campo: 'database',
    etiqueta: 'PostgreSQL',
    ayuda: '5433 para no chocar con otro PostgreSQL del equipo.',
  },
];

export function RutasYPuertos({ settings, onCambiar, onAtras, onContinuar }: Props) {
  const [ocupados, setOcupados] = useState<Partial<Record<keyof Ports, number | null>>>({});

  // Comprobar según se escribe evita el peor final posible: descubrir el
  // conflicto de puertos cuando ya se han descargado 500 MB.
  useEffect(() => {
    let cancelado = false;
    const comprobar = async () => {
      const resultado: Partial<Record<keyof Ports, number | null>> = {};
      for (const { campo } of PUERTOS) {
        const puerto = settings.ports[campo];
        const libre = await comprobarPuerto(puerto);
        if (!libre) resultado[campo] = await sugerirPuerto(puerto + 1);
      }
      if (!cancelado) setOcupados(resultado);
    };
    const temporizador = setTimeout(comprobar, 300);
    return () => {
      cancelado = true;
      clearTimeout(temporizador);
    };
  }, [settings.ports]);

  const cambiarPuerto = (campo: keyof Ports, valor: string) => {
    const numero = Number.parseInt(valor, 10);
    onCambiar({
      ports: { ...settings.ports, [campo]: Number.isNaN(numero) ? 0 : numero },
    });
  };

  const hayConflictos = Object.keys(ocupados).length > 0;

  return (
    <div className="mx-auto max-w-3xl">
      <PageHeader
        title="Dónde y en qué puertos"
        subtitle="Los valores por defecto sirven para casi todas las instalaciones."
      />

      <Card className="mt-6" title="Carpetas">
        <div className="grid gap-4">
          <Input
            label="Programa"
            value={settings.programDir ?? ''}
            onChange={(e) => onCambiar({ programDir: e.target.value })}
            helperText="Aquí van los binarios; se reemplazan en cada actualización."
          />
          <Input
            label="Datos"
            value={settings.dataDir ?? ''}
            onChange={(e) => onCambiar({ dataDir: e.target.value })}
            helperText="Base de datos, adjuntos, plugins y copias. Esto no se toca al actualizar."
          />
        </div>
      </Card>

      <Card className="mt-4" title="Puertos">
        <div className="grid gap-4 sm:grid-cols-3">
          {PUERTOS.map(({ campo, etiqueta, ayuda }) => {
            const alternativo = ocupados[campo];
            const ocupado = campo in ocupados;
            return (
              <div key={campo}>
                <Input
                  label={etiqueta}
                  type="number"
                  value={String(settings.ports[campo])}
                  onChange={(e) => cambiarPuerto(campo, e.target.value)}
                  status={ocupado ? 'error' : 'default'}
                  error={ocupado ? 'Ocupado por otro programa' : undefined}
                  helperText={ocupado ? undefined : ayuda}
                />
                {ocupado && alternativo && (
                  <Button
                    variant="ghost"
                    size="sm"
                    className="mt-1"
                    onClick={() => cambiarPuerto(campo, String(alternativo))}
                  >
                    Usar el {alternativo}
                  </Button>
                )}
              </div>
            );
          })}
        </div>
      </Card>

      <Navegacion
        onAtras={onAtras}
        onContinuar={onContinuar}
        continuarDeshabilitado={hayConflictos}
        motivo="Hay puertos ocupados"
      />
    </div>
  );
}
