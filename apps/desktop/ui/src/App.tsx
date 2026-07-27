import { Button, Card, Input, Loader, PageHeader } from '@openfactu/ui';
import { invoke } from '@tauri-apps/api/core';
import { AlertTriangle, PlugZap, Server } from 'lucide-react';
import { useEffect, useState } from 'react';

interface Config {
  serverUrl: string | null;
}

type Estado = 'cargando' | 'conexion' | 'conectando';

export function App() {
  const [estado, setEstado] = useState<Estado>('cargando');
  const [url, setUrl] = useState('');
  const [error, setError] = useState<string | null>(null);

  // Al arrancar, si ya hay un servidor guardado se entra directamente: quien
  // usa el ERP a diario no tiene por qué ver una pantalla de configuración.
  useEffect(() => {
    invoke<Config>('configuracion')
      .then((config) => {
        if (config.serverUrl) {
          setUrl(config.serverUrl);
          return conectar(config.serverUrl, true);
        }
        setEstado('conexion');
        return undefined;
      })
      .catch(() => setEstado('conexion'));
    // Sólo en el primer render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const conectar = async (direccion: string, silencioso = false) => {
    setEstado('conectando');
    setError(null);
    try {
      const vivo = await invoke<boolean>('probar_servidor', { url: direccion });
      if (!vivo) {
        // Se distingue «no responde» de «error al conectar»: casi siempre es
        // el servidor apagado o una IP mal escrita, y decirlo ahorra tiempo.
        setError(
          'No responde ningún Keirost en esa dirección. Comprueba que el equipo servidor está encendido y que la dirección es correcta.',
        );
        setEstado('conexion');
        return;
      }

      const destino = await invoke<string>('conectar', { url: direccion });
      window.location.replace(destino);
    } catch (e) {
      setError(String(e));
      setEstado('conexion');
      if (silencioso) setEstado('conexion');
    }
  };

  if (estado === 'cargando' || estado === 'conectando') {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-4">
        <Loader />
        <p className="text-sm text-[var(--fg-subtle)]">
          {estado === 'conectando' ? 'Conectando con Keirost…' : 'Abriendo Keirost…'}
        </p>
      </div>
    );
  }

  return (
    <div className="mx-auto flex h-full max-w-lg flex-col justify-center px-8">
      <PageHeader
        icon={<Server className="h-7 w-7" />}
        title="Conectar con Keirost"
        subtitle="Indica dónde está el Keirost de tu empresa. Sólo hay que hacerlo una vez."
      />

      <Card className="mt-6">
        <Input
          label="Dirección del servidor"
          placeholder="192.168.1.50:8080"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && conectar(url)}
          helperText="La misma que usáis en el navegador. Si Keirost está en este equipo: localhost:8080"
          error={error ?? undefined}
        />

        <Button className="mt-4 w-full" onClick={() => conectar(url)} disabled={!url.trim()}>
          <PlugZap className="mr-2 h-4 w-4" />
          Conectar
        </Button>
      </Card>

      {error && (
        <div className="mt-4 flex items-start gap-2 text-sm text-[var(--fg-subtle)]">
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-[var(--k-warning)]" />
          <span>
            Si aún no hay ningún Keirost instalado, ejecuta Keirost Setup en el equipo que vaya a
            hacer de servidor.
          </span>
        </div>
      )}
    </div>
  );
}
