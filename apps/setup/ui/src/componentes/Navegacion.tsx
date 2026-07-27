import { Button } from '@openfactu/ui';
import { ArrowLeft, ArrowRight } from 'lucide-react';

interface Props {
  onAtras?: () => void;
  onContinuar?: () => void;
  textoContinuar?: string;
  continuarDeshabilitado?: boolean;
  /** Motivo por el que no se puede continuar; se muestra junto al botón. */
  motivo?: string;
}

/** Barra inferior común a todos los pasos del asistente. */
export function Navegacion({
  onAtras,
  onContinuar,
  textoContinuar = 'Continuar',
  continuarDeshabilitado = false,
  motivo,
}: Props) {
  return (
    <div className="mt-8 flex items-center justify-between gap-4 border-t border-[var(--border-default)] pt-5">
      <div>
        {onAtras && (
          <Button variant="ghost" onClick={onAtras}>
            <ArrowLeft className="mr-2 h-4 w-4" />
            Atrás
          </Button>
        )}
      </div>

      <div className="flex items-center gap-3">
        {/* El motivo se enseña siempre que el botón esté bloqueado: un botón
            gris sin explicación es la forma más rápida de atascar a alguien. */}
        {continuarDeshabilitado && motivo && (
          <span className="text-sm text-[var(--fg-subtle)]">{motivo}</span>
        )}
        {onContinuar && (
          <Button onClick={onContinuar} disabled={continuarDeshabilitado}>
            {textoContinuar}
            <ArrowRight className="ml-2 h-4 w-4" />
          </Button>
        )}
      </div>
    </div>
  );
}
