import { Card, PageHeader } from '@openfactu/ui';
import { Database, MonitorSmartphone, ShieldAlert, Server } from 'lucide-react';

import { Navegacion } from '../componentes/Navegacion';

interface Props {
  administrador: boolean;
  onContinuar: () => void;
}

export function Bienvenida({ administrador, onContinuar }: Props) {
  return (
    <div className="mx-auto max-w-3xl">
      <PageHeader
        eyebrow="Instalador"
        title="Keirost"
        subtitle="Instala el ERP en este equipo: base de datos, servidor y aplicación, sin Docker y como servicios de Windows."
      />

      <div className="mt-6 grid gap-4 sm:grid-cols-3">
        <Card title="Base de datos" className="h-full">
          <div className="flex items-start gap-3 text-sm text-[var(--fg-subtle)]">
            <Database className="mt-0.5 h-5 w-5 shrink-0 text-accent" />
            <span>PostgreSQL propio, en su puerto, sin tocar el que ya tengas.</span>
          </div>
        </Card>
        <Card title="Servidor" className="h-full">
          <div className="flex items-start gap-3 text-sm text-[var(--fg-subtle)]">
            <Server className="mt-0.5 h-5 w-5 shrink-0 text-accent" />
            <span>Arranca solo con el equipo y se reinicia si falla.</span>
          </div>
        </Card>
        <Card title="Acceso" className="h-full">
          <div className="flex items-start gap-3 text-sm text-[var(--fg-subtle)]">
            <MonitorSmartphone className="mt-0.5 h-5 w-5 shrink-0 text-accent" />
            <span>Desde la aplicación y desde el navegador del resto de la oficina.</span>
          </div>
        </Card>
      </div>

      {!administrador && (
        // Registrar servicios exige elevación: es mejor decirlo ahora que
        // dejar que falle a mitad de instalación.
        <Card className="mt-5 border-[var(--k-warning)]">
          <div className="flex items-start gap-3">
            <ShieldAlert className="mt-0.5 h-5 w-5 shrink-0 text-[var(--k-warning)]" />
            <div className="text-sm">
              <p className="font-medium">Hace falta ejecutar como administrador</p>
              <p className="mt-1 text-[var(--fg-subtle)]">
                Keirost registra servicios de Windows y para eso necesita permisos elevados.
                Cierra esta ventana y vuelve a abrir Keirost Setup con «Ejecutar como
                administrador».
              </p>
            </div>
          </div>
        </Card>
      )}

      <Navegacion
        onContinuar={onContinuar}
        textoContinuar="Empezar"
        continuarDeshabilitado={!administrador}
        motivo={!administrador ? 'Reinicia el instalador como administrador' : undefined}
      />
    </div>
  );
}
