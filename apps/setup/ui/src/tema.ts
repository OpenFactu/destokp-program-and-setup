/**
 * Tema claro/oscuro.
 *
 * Por defecto se sigue lo que tenga configurado Windows: un instalador que sale
 * en blanco brillante en un equipo con tema oscuro se nota, y a esa hora de la
 * tarde molesta. La elección se guarda para el resto de la instalación y para
 * cuando se vuelva a abrir en modo gestor.
 */
import { applyTheme } from '@openfactu/ui';
import { useCallback, useEffect, useState } from 'react';

export type Preferencia = 'claro' | 'oscuro' | 'sistema';

const CLAVE = 'keirost:tema';

/** Colores del preset «Keirost Clásico»; el modo es lo único que cambia. */
const COLORES = { primary: '#0A1628', accent: '#0D9488' };

const consultaOscuro = () =>
  typeof window !== 'undefined' && typeof window.matchMedia === 'function'
    ? window.matchMedia('(prefers-color-scheme: dark)')
    : null;

export function preferenciaGuardada(): Preferencia {
  const guardada = typeof localStorage !== 'undefined' ? localStorage.getItem(CLAVE) : null;
  return guardada === 'claro' || guardada === 'oscuro' ? guardada : 'sistema';
}

/** Modo efectivo: resuelve «sistema» contra la configuración de Windows. */
export function modoEfectivo(preferencia: Preferencia): 'light' | 'dark' {
  if (preferencia === 'claro') return 'light';
  if (preferencia === 'oscuro') return 'dark';
  return consultaOscuro()?.matches ? 'dark' : 'light';
}

/** Aplica el tema. Se llama antes de montar React para que no haya parpadeo. */
export function aplicar(preferencia: Preferencia): void {
  applyTheme({ mode: modoEfectivo(preferencia), colors: COLORES });
}

/**
 * Estado del tema para la interfaz.
 *
 * Mientras la preferencia sea «sistema», se sigue en vivo el cambio de Windows:
 * quien tenga el tema automático por horario ve cambiar el instalador con el
 * resto del escritorio.
 */
export function useTema(): [Preferencia, (preferencia: Preferencia) => void] {
  const [preferencia, setPreferencia] = useState<Preferencia>(preferenciaGuardada);

  const cambiar = useCallback((nueva: Preferencia) => {
    setPreferencia(nueva);
    aplicar(nueva);
    try {
      if (nueva === 'sistema') localStorage.removeItem(CLAVE);
      else localStorage.setItem(CLAVE, nueva);
    } catch {
      // Sin almacenamiento (ventana en modo privado o restringido) el tema
      // sigue funcionando: sólo no se recuerda.
    }
  }, []);

  useEffect(() => {
    if (preferencia !== 'sistema') return;
    const consulta = consultaOscuro();
    if (!consulta) return;

    const alCambiar = () => aplicar('sistema');
    consulta.addEventListener('change', alCambiar);
    return () => consulta.removeEventListener('change', alCambiar);
  }, [preferencia]);

  return [preferencia, cambiar];
}
