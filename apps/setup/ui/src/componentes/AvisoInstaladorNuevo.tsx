import { Button } from '@openfactu/ui';
import { ArrowUpCircle, ExternalLink } from 'lucide-react';
import { useEffect, useState } from 'react';

import { abrirUrl, instaladorMasNuevo, type InstaladorNuevo } from '../api';

/**
 * Aviso de que hay un instalador más nuevo publicado.
 *
 * El asistente no se actualiza solo: quien lo tiene instalado se queda con el
 * que le llegó el primer día y no se entera de que hay uno mejor —ni siquiera
 * cuando el suyo ya no entiende el formato del manifest y se niega a seguir—.
 * Esto no lo actualiza, pero al menos lo dice y lleva a donde descargarlo.
 */
export function AvisoInstaladorNuevo() {
  const [nuevo, setNuevo] = useState<InstaladorNuevo | null>(null);
  const [cerrado, setCerrado] = useState(false);

  useEffect(() => {
    // Sin conexión esto devuelve null y no pasa nada: instalar no depende de
    // poder consultarlo.
    instaladorMasNuevo().then(setNuevo).catch(() => setNuevo(null));
  }, []);

  if (!nuevo || cerrado) return null;

  return (
    <div className="flex items-center gap-3 border-b border-[var(--border-default)] bg-[var(--bg-muted)] px-8 py-3 text-sm">
      <ArrowUpCircle className="h-5 w-5 shrink-0 text-accent" />
      <p className="flex-1 text-[var(--fg-subtle)]">
        Hay una versión más reciente de Keirost Setup: la{' '}
        <span className="font-medium text-[var(--fg-default)]">{nuevo.version}</span>. Estás
        usando la {nuevo.actual}.
      </p>
      <Button variant="secondary" size="sm" onClick={() => void abrirUrl(nuevo.url)}>
        Descargarla
        <ExternalLink className="ml-2 h-4 w-4" />
      </Button>
      {/* Se puede seguir con el que hay: casi siempre instala igual de bien, y
          obligar a descargar otro antes de empezar sería peor que el problema. */}
      <button
        type="button"
        onClick={() => setCerrado(true)}
        className="text-xs text-[var(--fg-subtle)] underline"
      >
        Seguir con ésta
      </button>
    </div>
  );
}
