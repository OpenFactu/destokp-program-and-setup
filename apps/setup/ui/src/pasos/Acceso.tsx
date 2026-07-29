import { Card, Input, PageHeader, Select } from '@openfactu/ui';
import { Cloud, Globe, ShieldCheck } from 'lucide-react';

import type { Acceso as AccesoConfig, Settings } from '../api';
import { Navegacion } from '../componentes/Navegacion';

interface Props {
  settings: Settings;
  onCambiar: (cambios: Partial<Settings>) => void;
  onAtras: () => void;
  onContinuar: () => void;
}

const MODOS: Array<{
  id: AccesoConfig['modo'];
  titulo: string;
  descripcion: string;
  icono: typeof Cloud;
}> = [
  {
    id: 'propio',
    titulo: 'Sólo la red de la oficina',
    descripcion:
      'Keirost se cifra con un certificado que genera el propio instalador. No hace falta dominio ni internet, y nadie de fuera puede entrar.',
    icono: ShieldCheck,
  },
  {
    id: 'letsencrypt',
    titulo: "Con un dominio propio (Let's Encrypt)",
    descripcion:
      'Keirost sigue viviendo en la red de la oficina, pero con un certificado que reconocen todos los navegadores. Ningún equipo tendrá que instalar nada. Hace falta un dominio y poder demostrar que es tuyo.',
    icono: Globe,
  },
  {
    id: 'tunel',
    titulo: 'Publicar en internet con Cloudflare Tunnel',
    descripcion:
      'Cloudflare pone el dominio y el certificado. No hay que abrir puertos, ni tener IP fija, ni renovar nada. A cambio, Keirost pasa a ser accesible desde fuera de la oficina.',
    icono: Cloud,
  },
];

export function Acceso({ settings, onCambiar, onAtras, onContinuar }: Props) {
  const acceso = settings.acceso;
  const cambiar = (cambios: Partial<AccesoConfig>) =>
    onCambiar({ acceso: { ...acceso, ...cambios } });

  // Sin token no hay túnel que valga: es lo único que Cloudflare necesita para
  // reconocer este equipo, y seguir sin él dejaría el servicio dando vueltas.
  const faltaToken =
    (acceso.modo === 'tunel' && acceso.token.trim().length === 0) ||
    (acceso.modo === 'letsencrypt' &&
      acceso.validacion === 'cloudflare' &&
      acceso.token.trim().length === 0);
  const faltaDominio = acceso.modo === 'letsencrypt' && acceso.dominio.trim().length === 0;
  const faltaCorreo = acceso.modo === 'letsencrypt' && !acceso.correo.includes('@');
  const incompleto = faltaToken || faltaDominio || faltaCorreo;
  const motivo = faltaDominio
    ? 'Escribe el dominio'
    : faltaCorreo
      ? 'Escribe un correo válido'
      : faltaToken
        ? 'Pega el token que te da Cloudflare'
        : undefined;

  return (
    <div className="mx-auto max-w-2xl">
      <PageHeader
        title="Cómo se entra a Keirost"
        subtitle="Siempre cifrado. Lo único que cambia es quién avala el certificado."
      />

      <div className="mt-6 grid gap-4">
        {MODOS.map(({ id, titulo, descripcion, icono: Icono }) => (
          <button
            key={id}
            type="button"
            className="w-full text-left"
            onClick={() => cambiar({ modo: id })}
          >
            <Card className={acceso.modo === id ? 'border-accent' : undefined}>
              <div className="flex items-start gap-4">
                <Icono className="mt-0.5 h-5 w-5 shrink-0 text-accent" />
                <div className="flex-1">
                  <p className="font-medium">{titulo}</p>
                  <p className="mt-1 text-sm text-[var(--fg-subtle)]">{descripcion}</p>
                </div>
                <input
                  type="radio"
                  name="modo-acceso"
                  className="mt-1 h-4 w-4 accent-[var(--accent)]"
                  checked={acceso.modo === id}
                  onChange={() => cambiar({ modo: id })}
                />
              </div>
            </Card>
          </button>
        ))}
      </div>

      {acceso.modo === 'propio' && (
        <Card className="mt-4" title="Lo que hay que saber">
          <p className="text-sm text-[var(--fg-subtle)]">
            En este equipo el navegador no dirá nada. En los demás avisará hasta que
            instales ahí el certificado, que el instalador deja preparado en la carpeta de
            datos. Es un fichero y dos clics por equipo.
          </p>
        </Card>
      )}

      {acceso.modo === 'letsencrypt' && (
        <Card className="mt-4" title="Datos del dominio">
          <div className="grid gap-4">
            <Input
              label="Dominio"
              placeholder="erp.miempresa.com"
              value={acceso.dominio}
              onChange={(e) => cambiar({ dominio: e.target.value })}
              helperText="Tiene que apuntar a este equipo. En una red interna, un registro A hacia su IP privada."
            />
            <Input
              label="Correo"
              type="email"
              placeholder="administracion@miempresa.com"
              value={acceso.correo}
              onChange={(e) => cambiar({ correo: e.target.value })}
              helperText="Let's Encrypt avisa aquí si el certificado deja de renovarse."
            />
            <Select
              label="Cómo se comprueba que el dominio es tuyo"
              value={acceso.validacion}
              onChange={(validacion) =>
                cambiar({ validacion: validacion as AccesoConfig['validacion'] })
              }
              options={[
                { value: 'cloudflare', label: 'Con la API de Cloudflare (recomendado)' },
                { value: 'puerto80', label: 'Por el puerto 80, si el servidor está publicado' },
              ]}
            />

            {acceso.validacion === 'cloudflare' && (
              <Input
                label="Token de la API de Cloudflare"
                type="password"
                placeholder="…"
                value={acceso.token}
                onChange={(e) => cambiar({ token: e.target.value })}
                helperText="Con permiso de edición de DNS sobre esa zona. Keirost pone y quita él solo el registro que pide Let's Encrypt."
              />
            )}
          </div>

          <p className="mt-4 text-sm text-[var(--fg-subtle)]">
            El certificado dura 90 días y se renueva solo a los 60, con una tarea diaria. Si
            eliges el puerto 80, el equipo tiene que ser accesible desde internet por ese
            puerto durante la comprobación.
          </p>
        </Card>
      )}

      {acceso.modo === 'tunel' && (
        <Card className="mt-4" title="Datos del túnel">
          <p className="text-sm text-[var(--fg-subtle)]">
            En el panel de Cloudflare (Zero Trust → Networks → Tunnels) crea un túnel,
            apúntalo a <span className="font-mono">https://localhost:{settings.ports.web}</span> y
            copia aquí el token que te da.
          </p>

          <div className="mt-4 grid gap-4">
            <Input
              label="Token del túnel"
              type="password"
              placeholder="eyJhIjoi…"
              value={acceso.token}
              onChange={(e) => cambiar({ token: e.target.value })}
              helperText="Queda guardado en el equipo, legible sólo para administradores."
            />
            <Input
              label="Dominio (opcional)"
              placeholder="erp.miempresa.com"
              value={acceso.dominio}
              onChange={(e) => cambiar({ dominio: e.target.value })}
              helperText="El que hayas configurado en Cloudflare. Sólo sirve para decírtelo al terminar."
            />
          </div>

          <p className="mt-4 text-sm text-[var(--fg-subtle)]">
            Con el túnel, Keirost queda accesible desde internet para quien tenga la
            dirección. Protégelo también con las políticas de acceso de Cloudflare si no
            quieres que dependa sólo de la contraseña.
          </p>
        </Card>
      )}

      <Navegacion
        onAtras={onAtras}
        onContinuar={onContinuar}
        continuarDeshabilitado={incompleto}
        motivo={motivo}
      />
    </div>
  );
}
