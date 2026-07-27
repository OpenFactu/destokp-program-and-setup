import { Badge, Button, Card, ConfirmDialog, PageHeader, Switch, useToast } from '@openfactu/ui';
import { ArrowUpCircle, PackagePlus, Trash2, Wrench } from 'lucide-react';
import { useEffect, useState } from 'react';

import { SelectorTema } from '../componentes/SelectorTema';
import {
  actualizar,
  consultarVersion,
  desinstalar,
  escucharInstalacion,
  reparar,
  type ExistingInstall,
  type ManifestSummary,
} from '../api';

interface Props {
  instalacion: ExistingInstall;
  administrador: boolean;
  /** Volver al asistente para instalar de cero (cambiar de perfil, por ejemplo). */
  onInstalarDeNuevo: () => void;
}

const PERFILES: Record<ExistingInstall['profile'], string> = {
  full: 'Completo',
  server: 'Sólo servidor',
  desktop: 'Sólo aplicación',
};

export function Gestor({ instalacion, administrador, onInstalarDeNuevo }: Props) {
  const [ultima, setUltima] = useState<ManifestSummary | null>(null);
  const [confirmandoBorrado, setConfirmandoBorrado] = useState(false);
  const [conservarDatos, setConservarDatos] = useState(true);
  const [enCurso, setEnCurso] = useState<string | null>(null);
  const [paso, setPaso] = useState<string | null>(null);
  const toast = useToast();

  useEffect(() => {
    consultarVersion('stable').then(setUltima).catch(() => setUltima(null));

    // Actualizar y reparar emiten los mismos eventos que la instalación: aquí
    // se enseña el paso en curso, que es lo que importa mientras se espera.
    let desuscribir: (() => void) | undefined;
    escucharInstalacion((evento) => {
      if (evento.kind === 'step') setPaso(evento.title);
      if (evento.kind === 'done') setPaso(null);
    }).then((fn) => {
      desuscribir = fn;
    });
    return () => desuscribir?.();
  }, []);

  const hayActualizacion = Boolean(ultima && ultima.version !== instalacion.version);

  /** Ejecuta una operación larga enseñando su progreso y su resultado. */
  const ejecutar = async (id: string, operacion: () => Promise<void>, exito: string) => {
    setEnCurso(id);
    try {
      await operacion();
      toast.success(exito);
    } catch (e) {
      toast.error(String(e));
    } finally {
      setEnCurso(null);
      setPaso(null);
    }
  };

  const acciones = [
    {
      id: 'actualizar',
      titulo: hayActualizacion ? `Actualizar a la ${ultima?.version}` : 'Buscar actualizaciones',
      descripcion: hayActualizacion
        ? 'Para los servicios, hace copia de la base de datos, reemplaza los programas y vuelve a arrancar.'
        : `Estás en la última versión publicada (${instalacion.version}).`,
      icono: ArrowUpCircle,
      variante: 'primary' as const,
      disponible: hayActualizacion,
      accion: () =>
        ejecutar(
          'actualizar',
          () => actualizar(),
          `Keirost actualizado a la ${ultima?.version ?? 'última versión'}`,
        ),
    },
    {
      id: 'reparar',
      titulo: 'Reparar',
      descripcion:
        'Vuelve a escribir la configuración y a registrar los servicios, sin tocar los datos. Es lo primero que probar si algo no arranca.',
      icono: Wrench,
      variante: 'secondary' as const,
      disponible: true,
      accion: () => ejecutar('reparar', () => reparar(), 'Keirost reparado y en marcha'),
    },
    {
      id: 'componentes',
      titulo: 'Cambiar componentes',
      descripcion: 'Añadir o quitar copias automáticas, IA local o monitorización.',
      icono: PackagePlus,
      variante: 'secondary' as const,
      disponible: true,
      accion: onInstalarDeNuevo,
    },
  ];

  return (
    <div className="mx-auto max-w-3xl px-8 py-8">
      <PageHeader
        eyebrow="Instalación existente"
        title="Keirost está instalado en este equipo"
        subtitle={`Versión ${instalacion.version} · ${PERFILES[instalacion.profile]} · instalado el ${new Date(
          instalacion.installedAt,
        ).toLocaleDateString('es-ES')}`}
        actions={
          <div className="flex items-center gap-3">
            {hayActualizacion && <Badge variant="accent">Hay novedades</Badge>}
            <SelectorTema />
          </div>
        }
      />

      {!administrador && (
        <Card className="mt-5 border-[var(--k-warning)]">
          <p className="text-sm">
            Abre Keirost Setup como administrador para poder actualizar, reparar o desinstalar.
          </p>
        </Card>
      )}

      {paso && (
        // Sin esto, actualizar parece que no hace nada durante varios minutos.
        <Card className="mt-5">
          <p className="text-sm">
            <span className="font-medium">{paso}</span>
            <span className="text-[var(--fg-subtle)]"> · no cierres esta ventana</span>
          </p>
        </Card>
      )}

      <div className="mt-6 grid gap-4">
        {acciones.map(({ id, titulo, descripcion, icono: Icono, variante, disponible, accion }) => (
          <Card key={id}>
            <div className="flex items-start gap-4">
              <Icono className="mt-0.5 h-5 w-5 shrink-0 text-accent" />
              <div className="flex-1">
                <p className="font-medium">{titulo}</p>
                <p className="mt-1 text-sm text-[var(--fg-subtle)]">{descripcion}</p>
              </div>
              <Button
                variant={variante}
                size="sm"
                disabled={!administrador || !disponible || Boolean(enCurso)}
                isLoading={enCurso === id}
                onClick={accion}
              >
                {id === 'actualizar' ? 'Actualizar' : id === 'reparar' ? 'Reparar' : 'Abrir'}
              </Button>
            </div>
          </Card>
        ))}

        <Card>
          <div className="flex items-start gap-4">
            <Trash2 className="mt-0.5 h-5 w-5 shrink-0 text-[var(--k-danger)]" />
            <div className="flex-1">
              <p className="font-medium">Desinstalar</p>
              <p className="mt-1 text-sm text-[var(--fg-subtle)]">
                Quita los servicios y los programas de este equipo.
              </p>
              <div className="mt-3">
                <Switch
                  id="conservar-datos"
                  checked={conservarDatos}
                  onChange={setConservarDatos}
                  label="Conservar la base de datos, los adjuntos y las copias"
                />
              </div>
            </div>
            <Button
              variant="danger"
              size="sm"
              disabled={!administrador}
              onClick={() => setConfirmandoBorrado(true)}
            >
              Desinstalar
            </Button>
          </div>
        </Card>
      </div>

      <ConfirmDialog
        open={confirmandoBorrado}
        onCancel={() => setConfirmandoBorrado(false)}
        title="¿Desinstalar Keirost?"
        // Sin los datos no hay vuelta atrás: la confirmación tiene que decir
        // exactamente qué va a desaparecer.
        message={
          conservarDatos
            ? 'Se quitarán los servicios y los programas. La base de datos, los adjuntos y las copias se quedan en el disco.'
            : 'Se borrará TODO, incluida la base de datos con la contabilidad y los adjuntos. Esto no se puede deshacer.'
        }
        confirmLabel="Desinstalar"
        tone={conservarDatos ? 'warning' : 'danger'}
        onConfirm={async () => {
          try {
            await desinstalar(conservarDatos);
            toast.success('Keirost se ha desinstalado');
          } catch (e) {
            toast.error(String(e));
          } finally {
            setConfirmandoBorrado(false);
          }
        }}
      />
    </div>
  );
}
