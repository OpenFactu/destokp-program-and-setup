import { Button, Card, Input, PageHeader, PasswordInput } from '@openfactu/ui';
import { KeyRound, RefreshCw } from 'lucide-react';
import { useEffect } from 'react';

import { generarContrasena, type Settings } from '../api';
import { Navegacion } from '../componentes/Navegacion';

interface Props {
  settings: Settings;
  onCambiar: (cambios: Partial<Settings>) => void;
  onAtras: () => void;
  onContinuar: () => void;
}

const MINIMO_ADMIN = 8;

/// Las mismas reglas que aplica el motor, para no dejar avanzar y fallar luego
/// al crear la base con medio cluster ya montado.
const identificadorInvalido = (valor: string) =>
  valor && !/^[A-Za-z][A-Za-z0-9_]{0,62}$/.test(valor)
    ? 'Empieza por letra y usa sólo letras, números y guiones bajos'
    : undefined;

export function Credenciales({ settings, onCambiar, onAtras, onContinuar }: Props) {
  // La contraseña de la base de datos no la teclea nadie: se genera fuerte y
  // se queda en el .env. Se muestra por si hace falta conectarse con otra
  // herramienta.
  useEffect(() => {
    if (!settings.databasePassword) {
      generarContrasena().then((clave) => onCambiar({ databasePassword: clave }));
    }
    // Sólo al entrar en el paso.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const adminCorta = settings.adminPassword.length < MINIMO_ADMIN;

  return (
    <div className="mx-auto max-w-2xl">
      <PageHeader
        title="Credenciales"
        subtitle="Sólo la del administrador hay que recordarla."
      />

      <Card className="mt-6" title="Administrador de Keirost" subtitle="Usuario «admin»">
        <PasswordInput
          label="Contraseña"
          value={settings.adminPassword}
          onChange={(e) => onCambiar({ adminPassword: e.target.value })}
          error={
            settings.adminPassword && adminCorta
              ? `Al menos ${MINIMO_ADMIN} caracteres`
              : undefined
          }
          helperText="Con esta contraseña se entra en Keirost la primera vez."
        />
      </Card>

      <Card
        className="mt-4"
        title="Base de datos"
        subtitle="Generada automáticamente; se guarda en la configuración del servidor"
        headerAction={
          <Button
            variant="ghost"
            size="sm"
            onClick={() => generarContrasena().then((c) => onCambiar({ databasePassword: c }))}
          >
            <RefreshCw className="mr-2 h-4 w-4" />
            Generar otra
          </Button>
        }
      >
        <PasswordInput
          label="Contraseña de PostgreSQL"
          value={settings.databasePassword}
          onChange={(e) => onCambiar({ databasePassword: e.target.value })}
          leftIcon={<KeyRound className="h-4 w-4" />}
          helperText="No hace falta memorizarla: Keirost la usa por dentro."
        />

        <div className="mt-4 grid gap-4 sm:grid-cols-2">
          <Input
            label="Nombre de la base"
            value={settings.databaseName}
            onChange={(e) => onCambiar({ databaseName: e.target.value })}
            error={identificadorInvalido(settings.databaseName)}
            helperText="Se crea con este nombre dentro del PostgreSQL de Keirost."
          />
          <Input
            label="Usuario"
            value={settings.databaseUser}
            onChange={(e) => onCambiar({ databaseUser: e.target.value })}
            error={identificadorInvalido(settings.databaseUser)}
            helperText="El dueño de esa base. No es el usuario con el que entras a Keirost."
          />
        </div>
      </Card>

      <Navegacion
        onAtras={onAtras}
        onContinuar={onContinuar}
        continuarDeshabilitado={adminCorta || !settings.databasePassword}
        motivo={`La contraseña del administrador necesita ${MINIMO_ADMIN} caracteres`}
      />
    </div>
  );
}
